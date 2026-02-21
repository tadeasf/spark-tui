# Analyze Module

**Path:** `src/analyze/`

Contains all performance analysis logic: suspect detection, bottleneck classification, and SQL correlation.

## Files

| File | Purpose |
|------|---------|
| `types.rs` | Core types: `Suspect`, `Severity`, `SuspectCategory`, `BottleneckPattern`, `RankedJob`, `SqlJobLink` |
| `skew.rs` | Data skew detection using task-level metrics |
| `suspects.rs` | Slow stage + spill detection, bottleneck classification, aggregation |
| `sql_linker.rs` | Cross-reference maps between jobs, stages, and SQL executions |

## `types.rs` — Core Types

### `Severity`

```rust
pub enum Severity {
    Warning,
    Critical,
}
```

Implements `Ord` for sorting (Critical > Warning).

### `SuspectCategory`

```rust
pub enum SuspectCategory {
    SlowStage,
    DataSkew,
    DataSizeSkew,
    RecordCountSkew,
    DiskSpill,
    CpuBottleneck,
    IoBottleneck,
    RecordExplosion,
    TaskFailures,
    MemoryPressure,
    ExecutorHotspot,
}
```

### `BottleneckPattern`

```rust
pub enum BottleneckPattern {
    LargeScan,
    WideShuffle,
    DataExplosion,
    RecordExplosion,
}
```

### `Suspect`

```rust
pub struct Suspect {
    pub severity: Severity,
    pub category: SuspectCategory,
    pub stage_id: i64,
    pub job_id: Option<i64>,
    pub title: String,
    pub detail: String,
    pub stage_name: Option<String>,
    pub sql_id: Option<i64>,
    pub sql_description: Option<String>,
    pub io_summary: Option<String>,
    pub recommendation: Option<String>,
    pub bottleneck: Option<BottleneckPattern>,
    pub sql_plan_hint: Option<String>,
}
```

### `RankedJob`

Processed job data for display, sorted by duration (running first, then slowest first).

```rust
pub struct RankedJob {
    pub job_id: i64,
    pub name: String,
    pub status: String,
    pub duration_ms: Option<i64>,
    pub num_tasks: i32,
    pub num_failed_tasks: i32,
    pub sql_id: Option<i64>,
    pub sql_description: Option<String>,
    pub stage_ids: Vec<i64>,
    pub submission_time: Option<String>,
    pub sql_plan: Option<String>,
}
```

### `HealthSummary`

```rust
pub struct HealthSummary {
    pub total_jobs: usize,
    pub running_jobs: usize,
    pub failed_jobs: usize,
    pub total_input_bytes: i64,
    pub total_output_bytes: i64,
    pub total_shuffle_bytes: i64,
    pub critical_count: usize,
    pub warning_count: usize,
    pub top_issues: Vec<String>,
}
```

Aggregates health metrics for the summary bar widget, computed by `compute_health_summary` in the poller.

## `skew.rs` — Skew Detection

### `detect_skew`

```rust
pub fn detect_skew(
    tasks: &[SparkTask],
    stage_id: i64,
    job_id: Option<i64>,
    stage_name: Option<&str>,
    sql_id: Option<i64>,
    sql_description: Option<&str>,
) -> Vec<Suspect>
```

Detects all forms of skew in a stage's tasks. Returns a `Vec<Suspect>` covering duration skew, data-size skew, record-count skew, and executor hotspot detection. See [Understanding Analysis](../analysis-guide.md) for threshold details.

## `suspects.rs` — Stage-Level Detection

### `detect_slow_stages`

```rust
pub fn detect_slow_stages(
    stages: &[SparkStage],
    stage_to_job: &HashMap<i64, i64>,
    job_to_sql: &HashMap<i64, i64>,
    sql_descriptions: &HashMap<i64, String>,
    sql_plans: &HashMap<i64, String>,
) -> Vec<Suspect>
```

Flags stages with `executor_run_time` exceeding `mean + 2*stddev` (warning) or `mean + 4*stddev` (critical).

### `detect_spill`

```rust
pub fn detect_spill(
    stages: &[SparkStage],
    stage_to_job: &HashMap<i64, i64>,
    job_to_sql: &HashMap<i64, i64>,
    sql_descriptions: &HashMap<i64, String>,
    sql_plans: &HashMap<i64, String>,
) -> Vec<Suspect>
```

Flags stages with `disk_bytes_spilled > 0` (warning) or `> 1 GB` (critical).

### `detect_cpu_efficiency`

```rust
pub fn detect_cpu_efficiency(
    stages: &[SparkStage],
    stage_to_job: &HashMap<i64, i64>,
    job_to_sql: &HashMap<i64, i64>,
    sql_descriptions: &HashMap<i64, String>,
    sql_plans: &HashMap<i64, String>,
) -> Vec<Suspect>
```

Detects CPU efficiency issues. Computes `cpu_ratio = (executor_cpu_time / 1_000_000) / executor_run_time`. Low ratio (< 0.3, runtime > 10s) → I/O bottleneck; high ratio (> 0.9, runtime > 30s) → CPU saturated.

### `detect_record_explosion`

```rust
pub fn detect_record_explosion(
    stages: &[SparkStage],
    stage_to_job: &HashMap<i64, i64>,
    job_to_sql: &HashMap<i64, i64>,
    sql_descriptions: &HashMap<i64, String>,
    sql_plans: &HashMap<i64, String>,
) -> Vec<Suspect>
```

Detects stages where `output_records > 10x input_records` (with `input_records > 1000`).

### `detect_task_failures`

```rust
pub fn detect_task_failures(
    stages: &[SparkStage],
    stage_to_job: &HashMap<i64, i64>,
    job_to_sql: &HashMap<i64, i64>,
    sql_descriptions: &HashMap<i64, String>,
    sql_plans: &HashMap<i64, String>,
) -> Vec<Suspect>
```

Detects stages with task failures or killed tasks.

### `detect_memory_pressure`

```rust
pub fn detect_memory_pressure(
    stages: &[SparkStage],
    stage_to_job: &HashMap<i64, i64>,
    job_to_sql: &HashMap<i64, i64>,
    sql_descriptions: &HashMap<i64, String>,
    sql_plans: &HashMap<i64, String>,
) -> Vec<Suspect>
```

Detects memory pressure: `memory_bytes_spilled > 50 MB` but `disk_bytes_spilled == 0`. This is a proactive warning before disk spill happens.

### `classify_bottleneck`

```rust
pub fn classify_bottleneck(s: &SparkStage) -> Option<BottleneckPattern>
```

Classifies root cause based on I/O patterns:

| Pattern | Condition |
|---------|-----------|
| DataExplosion | `input > 100 MB` and `output > 5x input` |
| LargeScan | `input > 1 GB` and `input > 10x (output + shuffle_write)` |
| WideShuffle | `shuffle_write > 500 MB` or `shuffle_read > input` |

### `aggregate_suspects`

```rust
pub fn aggregate_suspects(suspects: Vec<Suspect>) -> Vec<Suspect>
```

Sorts suspects by severity (Critical first).

### Helper Functions

| Function | Description |
|----------|-------------|
| `bottleneck_recommendation(b)` | Returns a recommendation string for a bottleneck pattern |
| `resolve_plan_hint(stage_id, ...)` | Extracts top SQL plan operations for a stage |
| `stage_io_summary(s)` | Formats I/O metrics for a stage |
| `resolve_sql(stage_id, ...)` | Resolves SQL ID and description for a stage |

## `sql_linker.rs` — Cross-Reference Maps

| Function | Signature | Description |
|----------|-----------|-------------|
| `build_job_to_sql_map` | `(sqls) -> HashMap<i64, i64>` | Maps job_id → sql_id from SQL execution job lists |
| `build_stage_to_job_map` | `(jobs) -> HashMap<i64, i64>` | Maps stage_id → job_id from job stage lists |
| `link_sql_to_jobs` | `(sqls) -> Vec<SqlJobLink>` | Groups SQL executions with their job IDs |
| `find_sql_for_job` | `(job_id, ...) -> (Option<i64>, Option<String>)` | Looks up SQL ID and description for a job |
| `stages_for_task_analysis` | `(stages) -> Vec<(i64, i64)>` | Selects up to ~15 stages for task-level analysis using multiple heuristics (top-by-runtime, top-by-shuffle, high-parallelism) |
