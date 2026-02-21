# Fetch Module

**Path:** `src/fetch/`

Handles HTTP communication with the Spark REST API via the Databricks driver proxy.

## Files

| File | Purpose |
|------|---------|
| `client.rs` | `SparkHttpClient` and `FetchError` |
| `types.rs` | Spark API response types (serde) |
| `spark.rs` | Endpoint methods on `SparkHttpClient` |
| `poller.rs` | Background polling loop and data aggregation |

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

## `poller.rs` — Background Poller

### `run_poller`

```rust
pub async fn run_poller(
    client: Arc<SparkHttpClient>,
    tx: mpsc::UnboundedSender<Action>,
    poll_interval: Duration,
)
```

1. Discovers the application ID
2. Enters a loop: `poll_once` → send result via channel → sleep

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
}
```

Note: `DataPayload` is defined in `src/tui/mod.rs` and contains all data needed to render the TUI.

**New fields in v2:**
- `stage_sql_hints` — maps `stage_id → String` with top SQL plan operations (e.g., "HashAggregate → Exchange → Scan parquet"), pre-computed for display in stage detail headers
- `critical_stages` — set of stage IDs that represent the critical path (longest wall-clock stage per job), used for "CP" annotations in the job detail view
