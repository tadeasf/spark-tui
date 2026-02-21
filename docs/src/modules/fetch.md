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
| `SparkTask` | `task_id`, `stage_id`, `executor_id`, `host`, `status`, `duration`, I/O bytes, spill bytes |
| `RawSparkTask` | Raw API format with nested `task_metrics` (flattened into `SparkTask` via custom deserializer) |

## `spark.rs` — Endpoint Methods

Methods on `SparkHttpClient`:

| Method | Path | Returns |
|--------|------|---------|
| `discover_app_id` | `/applications` | `String` (first application ID) |
| `get_jobs` | `/applications/{id}/jobs` | `Vec<SparkJob>` |
| `get_stages` | `/applications/{id}/stages` | `Vec<SparkStage>` |
| `get_sql_executions` | `/applications/{id}/sql` | `Vec<SparkSqlExecution>` |
| `get_task_list` | `/applications/{id}/stages/{sid}/{attempt}/taskList` | `Vec<SparkTask>` |

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

1. Fetch jobs, stages, SQL executions concurrently (`tokio::join!`)
2. Build cross-reference maps (job↔SQL, stage↔job)
3. Build ranked jobs (sorted by duration, running first)
4. Run stage-level analysis (slow stages, spill)
5. Fetch task lists for top 5 slowest stages
6. Run skew detection on fetched tasks
7. Aggregate and sort suspects
8. Return `DataPayload`
