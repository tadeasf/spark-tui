# Fetch Module

**Path:** `src/fetch/`

Handles HTTP communication with the Spark REST API via the Databricks driver proxy.

## Files

| File | Purpose |
|------|---------|
| `client.rs` | `SparkHttpClient` and `FetchError` |
| `types.rs` | Spark API response types (serde) |
| `spark.rs` | Endpoint methods on `SparkHttpClient` |
| `databricks.rs` | `DatabricksClient` for Databricks REST API, DBFS, Spark UI, and History Server |
| `poller.rs` | Background polling loop, historical fallback chain, and data aggregation |
| `eventlog/` | Event log parsing: DBFS download, gzip decompression, `SparkEvent` serde |

## `client.rs` — SparkHttpClient

### `SparkHttpClient`

```rust
pub struct SparkHttpClient {
    client: reqwest::Client,
    base_url: String,
    token: String,
}
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `new` | `(base_url, token) -> Self` | Creates a new client |
| `base_url` | `&self -> &str` | Returns the base URL |
| `get` | `&self, path: &str -> Result<T, FetchError>` | Generic GET request with Bearer auth and JSON deserialization |

### `FetchError`

Error enum with user-friendly messages for common HTTP errors:

| Variant | Status | Message |
|---------|--------|---------|
| `Unauthorized` | 401 | Token expired or invalid |
| `Forbidden` | 403 | Insufficient permissions |
| `NotFound` | 404 | Spark UI not available / app may have ended |
| `ServiceUnavailable` | 503 | Cluster not reachable |
| `HttpError` | other | Generic HTTP error with status and body |
| `Deserialize` | — | JSON deserialization failure |
| `Request` | — | Network-level request failure |
| `NoApplications` | — | No Spark applications found |

## `types.rs` — API Types

### Application & Job Types

| Type | Key Fields |
|------|------------|
| `SparkApplication` | `id`, `name` |
| `SparkJob` | `job_id`, `name`, `status`, `submission_time`, `completion_time`, `stage_ids`, task counts |
| `JobStatus` | `Succeeded`, `Running`, `Failed`, `Unknown` |

### Stage Types

| Type | Key Fields |
|------|------------|
| `SparkStage` | `stage_id`, `attempt_id`, `status`, `num_tasks`, `executor_run_time`, I/O bytes, spill bytes |
| `StageStatus` | `Active`, `Complete`, `Pending`, `Failed`, `Skipped` |

### SQL Types

| Type | Key Fields |
|------|------------|
| `SparkSqlExecution` | `id`, `status`, `description`, `plan_description`, `duration`, job ID lists |

### Task Types

| Type | Key Fields |
|------|------------|
| `SparkTask` | `task_id`, `stage_id`, `executor_id`, `host`, `status`, `duration`, I/O bytes, spill bytes, `peak_execution_memory` |
| `RawSparkTask` | Raw API format with nested `task_metrics` (flattened into `SparkTask` via custom deserializer) |

### Executor Types

| Type | Key Fields |
|------|------------|
| `SparkExecutor` | `id`, `total_cores`, `max_memory`, `is_active` |
| `ClusterResources` | `total_executor_memory`, `total_executor_cores`, `num_executors` |

## `spark.rs` — Endpoint Methods

Methods on `SparkHttpClient`:

| Method | Path | Returns |
|--------|------|---------|
| `discover_app_id` | `/applications` | `String` (first application ID) |
| `get_jobs` | `/applications/{id}/jobs` | `Vec<SparkJob>` |
| `get_stages` | `/applications/{id}/stages` | `Vec<SparkStage>` |
| `get_sql_executions` | `/applications/{id}/sql` | `Vec<SparkSqlExecution>` |
| `get_task_list` | `/applications/{id}/stages/{sid}/{attempt}/taskList` | `Vec<SparkTask>` |
| `get_executors` | `/applications/{id}/executors` | `Vec<SparkExecutor>` |

## `databricks.rs` — DatabricksClient

Thin client for Databricks REST API `/api/2.0/*` endpoints and workspace-level requests.

### `DatabricksClient`

```rust
pub struct DatabricksClient {
    client: reqwest::Client,
    base_url: String,        // e.g. https://{host}/api/2.0
    workspace_root: String,  // e.g. https://{host}
    token: String,
    sparkui_cookie: Option<String>,
}
```

### `SparkuiProbeResult`

Tri-state result from probing a Spark UI endpoint:

| Variant | Fields | Meaning |
|---------|--------|---------|
| `Ready` | `base_url`, `app_id` | Spark UI is serving JSON data |
| `Loading` | `base_url` | Spark UI is authenticated but still downloading/parsing event logs |
| `NotFound` | — | No accessible Spark UI endpoint found |

### Key Methods

| Method | Description |
|--------|-------------|
| `get_cluster_info` | Fetch cluster state, log config, and `spark_context_id` |
| `try_sparkui_endpoint` | Probe Historical Spark UI REST API (tries Bearer + cookie auth, workspace + dataplane domains) |
| `probe_sparkui_url` | Re-probe a single URL for retry after `Loading` state |
| `fetch_sparkui_data` | Fetch all data (jobs, stages, SQL, tasks) from a ready Spark UI endpoint |
| `discover_history_server` | Probe known Spark History Server proxy URL patterns |
| `fetch_history_data` | Fetch all data from History Server |
| `dbfs_list` / `dbfs_read_full` | DBFS file operations |
| `find_default_event_logs` | Scan well-known DBFS paths for event log files |

### Warm-up Detection

The `is_loading_page()` helper detects HTML loading pages returned during Spark UI warm-up by checking for patterns like `<title>Loading</title>`, `loading spark ui`, `please wait`, or generic HTML responses.

## `eventlog/` — Event Log Parsing

| File | Purpose |
|------|---------|
| `events.rs` | `SparkEvent` serde types for Spark event log JSON lines |
| `parser.rs` | `EventLogParser` — converts raw events into jobs, stages, SQL, tasks |
| `loader.rs` | DBFS download + gzip decompression pipeline |

The `load_event_log()` function orchestrates: discover log path → download from DBFS → decompress gzip → parse JSON lines → return structured data.

## `poller.rs` — Background Poller

### `run_poller`

```rust
pub async fn run_poller(
    client: Arc<SparkHttpClient>,
    databricks: Arc<DatabricksClient>,
    config: Arc<Config>,
    tx: mpsc::UnboundedSender<Action>,
    poll_interval: Duration,
)
```

1. Discovers the application ID (or detects cluster unreachable → historical fallback)
2. Enters a live poll loop: `poll_once` → send result via channel → sleep
3. On connection loss during polling, falls back to historical data

### Historical Fallback Chain

When the cluster is unreachable, `try_load_historical` attempts these strategies in order:

| Strategy | Function | Condition |
|----------|----------|-----------|
| 0 — Spark UI | `try_sparkui` | Requires `spark_context_id`; retries with backoff if UI is loading |
| 1 — History Server | `try_history_server` | Probes known proxy URL patterns |
| 2 — DBFS event logs | `try_dbfs_event_logs` | Uses `cluster_log_conf` or `--event-log-path` |
| 3 — Default paths | `find_default_event_logs` | Scans well-known DBFS directories |

The Spark UI strategy includes warm-up retry: if the endpoint returns an HTML loading page, it retries with backoff delays of 3, 5, 10, 15, 20 seconds (~53s total).

### `poll_once`

Fetches all data and runs analysis in a single poll cycle:

1. Fetch jobs, stages, SQL executions, and executors concurrently (4-way `tokio::join!`)
2. Aggregate active executors into `ClusterResources` (total memory, cores, executor count)
3. Build cross-reference maps (job↔SQL, stage↔job)
4. Build ranked jobs (sorted by duration, running first)
5. Create a `SuspectContext` from the cross-reference maps
6. Run 10 stage-level detectors via function pointer table (`detect_slow_stages`, `detect_spill`, `detect_cpu_efficiency`, `detect_record_explosion`, `detect_task_failures`, `detect_memory_pressure`, `detect_partition_count`, `detect_broadcast_join`, `detect_python_udf`, `detect_cache_opportunity`)
7. Fetch task lists for top ~15 stages (selected by multiple heuristics)
8. Run skew detection on fetched tasks (duration, data-size, record-count, executor hotspot)
9. Aggregate and sort suspects (severity first, then `estimated_savings_ms` descending)
10. Build `stage_sql_hints` — maps stage_id to top SQL plan operations
11. Compute `critical_stages` — the longest wall-clock stage per job (critical path)
12. Compute `HealthSummary` (job/IO counts, critical/warning counts, top issues)
13. Return `DataPayload`

### `compute_health_summary`

```rust
fn compute_health_summary(
    jobs: &[RankedJob],
    stages: &[SparkStage],
    suspects: &[Suspect],
) -> HealthSummary
```

Aggregates job counts, total I/O bytes, and suspect severity counts into a `HealthSummary` for the summary bar widget.

### `DataPayload`

```rust
pub struct DataPayload {
    pub app_id: String,
    pub jobs: Vec<RankedJob>,
    pub stages: Vec<SparkStage>,
    pub sql_executions: Vec<SparkSqlExecution>,
    pub suspects: Vec<Suspect>,
    pub stage_tasks: Arc<HashMap<i64, Vec<SparkTask>>>,
    pub summary: HealthSummary,
    pub cluster_resources: ClusterResources,
    pub stage_sql_hints: Arc<HashMap<i64, String>>,
    pub critical_stages: Arc<HashSet<i64>>,
    pub last_updated: String,
    pub data_source: DataSourceMode,
    pub data_source_detail: Option<String>,
}
```

Note: `DataPayload` is defined in `src/tui/mod.rs` and contains all data needed to render the TUI.

**Key fields:**
- `stage_sql_hints` — maps `stage_id → String` with top SQL plan operations (e.g., "HashAggregate → Exchange → Scan parquet"), pre-computed for display in stage detail headers
- `critical_stages` — set of stage IDs that represent the critical path (longest wall-clock stage per job), used for "CP" annotations in the job detail view
- `data_source` — `DataSourceMode::Live` or `DataSourceMode::Historical`, displayed in the status line
- `data_source_detail` — optional label like `"Spark UI"`, `"history server"`, or `"event logs"`, shown alongside the HISTORICAL badge
