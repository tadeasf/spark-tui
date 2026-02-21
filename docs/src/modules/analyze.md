# Analyze Module

**Path:** `src/analyze/`

Contains all performance analysis logic: suspect detection, bottleneck classification, and SQL correlation.

## Files

| File | Purpose |
|------|---------|
| `types.rs` | Core types: `Suspect`, `Severity`, `SuspectCategory`, `BottleneckPattern`, `RankedJob`, `SqlJobLink` |
| `skew.rs` | Data skew detection using task-level metrics |
| `suspects.rs` | `SuspectContext`, 10 stage-level detectors, bottleneck classification, aggregation |
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
    TooManyPartitions,
    TooFewPartitions,
    BroadcastJoinOpportunity,
    PythonUdf,
    CacheOpportunity,
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
    pub estimated_savings_ms: i64,
}
```

The `estimated_savings_ms` field contains a heuristic estimate of time savings from fixing the issue. It is used as a secondary sort key (after severity) in `aggregate_suspects`.

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

### Constants

| Constant | Value |
|----------|-------|
| `ONE_MB` | 1,048,576 bytes |
| `FIFTY_MB` | 52,428,800 bytes |
| `ONE_HUNDRED_MB` | 104,857,600 bytes |
| `FIVE_HUNDRED_MB` | 524,288,000 bytes |
| `ONE_GB` | 1,073,741,824 bytes |

### `SuspectContext`

Holds all lookup maps needed by suspect detectors, eliminating repetitive parameter passing.

```rust
pub struct SuspectContext<'a> {
    pub stage_to_job: &'a HashMap<i64, i64>,
    pub job_to_sql: &'a HashMap<i64, i64>,
    pub sql_descriptions: &'a HashMap<i64, String>,
    pub sql_plans: &'a HashMap<i64, String>,
}
```

**Constructor:**

```rust
pub fn new(
    stage_to_job: &'a HashMap<i64, i64>,
    job_to_sql: &'a HashMap<i64, i64>,
    sql_descriptions: &'a HashMap<i64, String>,
    sql_plans: &'a HashMap<i64, String>,
) -> Self
```

**Methods:**

| Method | Signature | Description |
|--------|-----------|-------------|
| `job_id` | `(&self, stage_id: i64) -> Option<i64>` | Look up the job_id for a stage |
| `resolve_sql` | `(&self, stage_id: i64) -> (Option<i64>, Option<String>)` | Resolve SQL id and description for a stage via its job (private) |
| `resolve_plan_hint_for` | `(&self, stage_id: i64) -> Option<String>` | Resolve top SQL plan operations for a stage (e.g., "HashAggregate → Exchange → Scan") |
| `enrich` | `(&self, suspect: &mut Suspect, stage: &SparkStage)` | Enrich a suspect with stage_name, SQL linkage, I/O summary, and plan hint |

### Stage-Level Detectors

All 10 stage-level detectors share the same signature:

```rust
pub fn detect_*(stages: &[SparkStage], ctx: &SuspectContext) -> Vec<Suspect>
```

They are dispatched via a function pointer table in the poller:

```rust
type DetectorFn = fn(&[SparkStage], &SuspectContext) -> Vec<Suspect>;
let detectors: &[DetectorFn] = &[
    detect_slow_stages,
    detect_spill,
    detect_cpu_efficiency,
    detect_record_explosion,
    detect_task_failures,
    detect_memory_pressure,
    detect_partition_count,
    detect_broadcast_join,
    detect_python_udf,
    detect_cache_opportunity,
];
```

### `detect_slow_stages`

Flags stages with `executor_run_time` exceeding `mean + 2*stddev` (warning) or `mean + 4*stddev` (critical). Sets `estimated_savings_ms` to time above mean.

### `detect_spill`

Flags stages with `disk_bytes_spilled > 0` (warning) or `> 1 GB` (critical). Estimates ~30% of runtime as spill overhead.

### `detect_cpu_efficiency`

Detects CPU efficiency issues. Computes `cpu_ratio = (executor_cpu_time / 1_000_000) / executor_run_time`. Low ratio (< 0.3, runtime > 10s) → I/O bottleneck; high ratio (> 0.9, runtime > 30s) → CPU saturated. Estimates ~20% savings.

### `detect_record_explosion`

Detects stages where `output_records > 10x input_records` (with `input_records > 1000`). Estimates ~50% savings.

### `detect_task_failures`

Detects stages with task failures or killed tasks. Estimates savings proportional to failure rate.

### `detect_memory_pressure`

Detects memory pressure: `memory_bytes_spilled > 50 MB` but `disk_bytes_spilled == 0`. Estimates ~10% savings from GC overhead.

### `detect_partition_count`

Detects partition count issues. Two sub-categories:

- **TooManyPartitions**: `num_tasks > 10,000` and `avg_bytes_per_task < 1 MB`. Estimates ~40% savings from scheduling overhead.
- **TooFewPartitions**: `num_tasks ≤ 8` and `avg_bytes_per_task > 1 GB`. Estimates ~50% savings from straggler elimination.

Recommendations include a computed target partition count for ~128 MB/partition.

### `detect_broadcast_join`

Detects shuffle joins where one side is small enough to broadcast. Triggers when `shuffle_write_bytes < 100 MB`, `executor_run_time > 5s`, and the SQL plan hint contains join indicators (`SortMerge`, `ShuffledHash`, `Join`). Estimates ~60% savings from shuffle elimination.

### `detect_python_udf`

Detects Python UDFs in SQL plans by searching for markers: `ArrowEvalPython`, `BatchEvalPython`, `PythonUDF`, `PythonRunner`. Escalates to Critical severity if also CPU-bound (ratio > 0.9, runtime > 30s). Estimates ~50% savings.

### `detect_cache_opportunity`

Detects repeated computations by grouping completed stages by cleaned name. Triggers when ≥ 2 stages share a name and total runtime > 30s. Savings estimated as `total_runtime - min_single_runtime`.

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
pub fn aggregate_suspects(mut suspects: Vec<Suspect>) -> Vec<Suspect>
```

Sorts suspects by severity (Critical first), then by `estimated_savings_ms` descending as a tiebreaker.

### Helper Functions

| Function | Description |
|----------|-------------|
| `classify_bottleneck(s)` | Classifies root-cause bottleneck pattern for a stage |
| `bottleneck_recommendation(b)` | Returns a PySpark-specific recommendation string for a bottleneck pattern |
| `stage_io_summary(s)` | Formats I/O metrics for a stage (including in:out ratio) |

Note: `resolve_sql` and `resolve_plan_hint_for` are now methods on `SuspectContext` (see above).

## `sql_linker.rs` — Cross-Reference Maps

| Function | Signature | Description |
|----------|-----------|-------------|
| `build_job_to_sql_map` | `(sqls) -> HashMap<i64, i64>` | Maps job_id → sql_id from SQL execution job lists |
| `build_stage_to_job_map` | `(jobs) -> HashMap<i64, i64>` | Maps stage_id → job_id from job stage lists |
| `link_sql_to_jobs` | `(sqls) -> Vec<SqlJobLink>` | Groups SQL executions with their job IDs |
| `find_sql_for_job` | `(job_id, ...) -> (Option<i64>, Option<String>)` | Looks up SQL ID and description for a job |
| `stages_for_task_analysis` | `(stages) -> Vec<(i64, i64)>` | Selects up to ~15 stages for task-level analysis using multiple heuristics (top-by-runtime, top-by-shuffle, high-parallelism) |
