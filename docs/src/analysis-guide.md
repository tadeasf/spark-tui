# Understanding Analysis

spark-tui automatically detects performance issues in your Spark application and presents them as **suspects**. This guide explains how each detector works, what the thresholds mean, and how to act on the findings.

## Suspect Categories

### Slow Stage

Detects stages whose `executor_run_time` is statistically anomalous compared to all completed stages.

**How it works:**

1. Computes the mean and standard deviation of `executor_run_time` across all completed stages
2. Flags stages that exceed the threshold

| Severity | Threshold |
|----------|-----------|
| Warning | `executor_run_time > mean + 2 * stddev` |
| Critical | `executor_run_time > mean + 4 * stddev` |

The suspect detail shows how many times slower the stage is compared to the average (e.g., "3.5x slower than average").

### Data Skew

Detects uneven task duration distribution within a stage, indicating skewed partitions.

**How it works:**

1. Collects all task durations for the stage
2. Computes the coefficient of variation (CV = stddev / mean) and the max/median ratio
3. Flags if either metric exceeds threshold

| Severity | Threshold |
|----------|-----------|
| Warning | CV > 1.0 **or** max > 3x median |
| Critical | CV > 2.0 **or** max > 10x median |

The suspect detail identifies the slowest task, its duration vs. the median, and how much data it processed.

**Note:** Task-level analysis is performed for up to ~15 stages selected by multiple heuristics (top-by-runtime, top-by-shuffle, high-parallelism). On-demand task fetching is triggered when entering StageDetail for stages not already analyzed.

### Data Size Skew

Detects uneven data size distribution across tasks within a stage.

**How it works:**

1. Computes the total bytes processed per task (`input_bytes + shuffle_read_bytes`)
2. Applies the same CV and max/median ratio thresholds as duration skew

| Severity | Threshold |
|----------|-----------|
| Warning | CV > 1.0 **or** max > 3x median |
| Critical | CV > 2.0 **or** max > 10x median |

The suspect detail identifies the task processing the most data, its byte count vs. the median.

### Record Count Skew

Detects uneven record count distribution across tasks within a stage.

**How it works:**

1. Computes the total records processed per task (`input_records + shuffle_read_records`)
2. Applies CV and max/median ratio thresholds (only when max records > 1000)

| Severity | Threshold |
|----------|-----------|
| Warning | CV > 1.0 **or** max > 3x median (and max > 1000) |
| Critical | CV > 2.0 **or** max > 10x median |

Indicates hot keys in joins or group-bys.

### Disk Spill

Detects stages where data was spilled from memory to disk, indicating insufficient executor memory.

**How it works:**

1. Checks `disk_bytes_spilled` for each stage
2. Any spill > 0 is flagged

| Severity | Threshold |
|----------|-----------|
| Warning | `disk_bytes_spilled > 0` |
| Critical | `disk_bytes_spilled > 1 GB` |

The suspect detail shows both memory spill and disk spill amounts.

### CPU Bottleneck

Detects stages where the CPU is fully saturated for a sustained period.

**How it works:**

1. Computes `cpu_ratio = (executor_cpu_time / 1_000_000) / executor_run_time`
2. Flags stages with high CPU ratio and significant runtime

| Severity | Threshold |
|----------|-----------|
| Warning | `cpu_ratio > 0.9` **and** `runtime > 30s` |

The suspect detail shows CPU time vs. runtime and utilization percentage.

### I/O Bottleneck

Detects stages that are I/O or GC bound (low CPU utilization despite significant runtime).

**How it works:**

1. Uses the same CPU ratio as CPU Bottleneck detection
2. Flags stages with low CPU ratio

| Severity | Threshold |
|----------|-----------|
| Warning | `cpu_ratio < 0.3` **and** `runtime > 10s` |

Consider increasing memory, improving data locality, using faster storage, or checking GC pauses.

### Record Explosion

Detects stages where output records vastly exceed input records, indicating `explode()`, cross joins, or `generate()` operations.

**How it works:**

1. Checks if `output_records > 10x input_records` (only when `input_records > 1000`)

| Severity | Threshold |
|----------|-----------|
| Warning | `output_records > 10x input_records` |
| Critical | `output_records > 100x input_records` |

### Task Failures

Detects stages with failed or killed tasks.

**How it works:**

1. Checks if `num_failed_tasks > 0` or `num_killed_tasks > 0`

| Severity | Threshold |
|----------|-----------|
| Warning | Any failed or killed tasks |
| Critical | Failure rate > 10% **or** total problematic > 10 |

Common causes include OOM, data corruption, and fetch failures.

### Memory Pressure

Detects stages where memory spill is occurring but hasn't yet reached disk — a proactive warning before disk spill happens.

**How it works:**

1. Checks if `memory_bytes_spilled > 50 MB` **and** `disk_bytes_spilled == 0`

| Severity | Threshold |
|----------|-----------|
| Warning | `memory_bytes_spilled > 50 MB` with no disk spill |

Recommendation: increase `spark.executor.memory` or `spark.executor.memoryOverhead`, reduce partition size.

### Executor Hotspot

Detects stages where a single executor handles a disproportionate share of data.

**How it works:**

1. Sums `input_bytes + shuffle_read_bytes` per executor
2. Flags executors processing > 50% of total data

| Severity | Threshold |
|----------|-----------|
| Warning | One executor handles > 50% of data |

Check data locality and partition assignment. This may indicate skewed partition-to-executor mapping.

### Too Many Partitions

Detects stages with excessive small partitions, causing high scheduling overhead.

**How it works:**

1. Computes `avg_bytes_per_task = (input_bytes + shuffle_read_bytes) / num_tasks`
2. Flags stages with too many tiny partitions

| Severity | Threshold |
|----------|-----------|
| Warning | `num_tasks > 10,000` **and** `avg_bytes_per_task < 1 MB` |

The recommendation suggests a target partition count to achieve ~128 MB/partition: `df.coalesce(N)`.

### Too Few Partitions

Detects stages with too few large partitions, causing stragglers and underutilized executors.

**How it works:**

1. Computes `avg_bytes_per_task = (input_bytes + shuffle_read_bytes) / num_tasks`
2. Flags stages with too few large partitions

| Severity | Threshold |
|----------|-----------|
| Warning | `num_tasks ≤ 8` **and** `avg_bytes_per_task > 1 GB` |

The recommendation suggests a target partition count: `df.repartition(N)`.

### Broadcast Join Opportunity

Detects shuffle joins where one side is small enough to broadcast, eliminating the shuffle entirely.

**How it works:**

1. Filters stages with `shuffle_write_bytes < 100 MB` and `executor_run_time > 5s`
2. Checks if the SQL plan contains join indicators (`SortMerge`, `ShuffledHash`, `Join`)

| Severity | Threshold |
|----------|-----------|
| Warning | `shuffle_write < 100 MB` **and** join detected in SQL plan |

**Recommendation:** `from pyspark.sql.functions import broadcast; df.join(broadcast(small_df), on='key')`.

### Python UDF

Detects Python UDF usage in SQL plans, which causes row-by-row serialization overhead.

**How it works:**

1. Searches the SQL plan hint for markers: `ArrowEvalPython`, `BatchEvalPython`, `PythonUDF`, `PythonRunner`
2. If the stage is also CPU-bound (ratio > 0.9, runtime > 30s), severity escalates to Critical

| Severity | Threshold |
|----------|-----------|
| Warning | Python UDF marker found in plan, `runtime > 5s` |
| Critical | Python UDF + CPU ratio > 0.9 + `runtime > 30s` |

**Recommendation:** Replace `@udf` with `@pandas_udf` for vectorized execution, or use native `F.when()`/`F.expr()` functions.

### Cache Opportunity

Detects repeated computations (stages with the same name) that could benefit from caching.

**How it works:**

1. Groups completed stages by cleaned name
2. Flags groups where ≥ 2 stages share a name and total runtime > 30s

| Severity | Threshold |
|----------|-----------|
| Warning | ≥ 2 stages with same name, total `runtime > 30s` |

**Recommendation:** `df.cache()` or `df.persist(StorageLevel.MEMORY_AND_DISK)` before the first action. Call `df.unpersist()` when no longer needed.

## Bottleneck Classification

When a slow stage or spill suspect is detected, spark-tui classifies the **root cause** based on I/O patterns:

| Pattern | Condition | Meaning |
|---------|-----------|---------|
| **Data Explosion** | `input > 100 MB` and `output > 5x input` | Stage produces far more data than it reads (e.g., `explode`, cross join) |
| **Large Scan** | `input > 1 GB` and `input > 10x (output + shuffle_write)` | Stage reads a lot but produces little (missing pushdown filters) |
| **Wide Shuffle** | `shuffle_write > 500 MB` or `shuffle_read > input` | Stage shuffles more data than it reads directly (broad join, groupBy on high-cardinality key) |
| **Record Explosion** | `output_records > 10x input_records` | Attached to record explosion suspects (see above) |

If none of these patterns match, no bottleneck tag is shown.

## Recommendations

All recommendations use PySpark-specific syntax for immediate applicability. Each suspect includes a recommendation based on its category and bottleneck pattern:

| Category + Bottleneck | Recommendation |
|-----------------------|----------------|
| Data Skew | Repartition or salt skewed keys |
| Data Size Skew | Repartition by a more uniform key or use salting |
| Record Count Skew | Check for hot keys in joins or group-bys |
| Disk Spill | `spark.conf.set('spark.executor.memory', '8g')` or `df.repartition(200)` |
| CPU Bottleneck | Replace `@udf` with `@pandas_udf` or native `F.when()`/`F.expr()`. Cache with `df.cache()` and increase parallelism |
| I/O Bottleneck | `spark.conf.set('spark.executor.memory', '8g')`, cache hot DataFrames with `df.cache()`, or use `df.repartition()` for better locality |
| Record Explosion | Filter before explode: `df.filter(...).select(explode('col'))`. Check for unintentional cross joins |
| Task Failures | Check executor logs for OOM/fetch failures. `spark.conf.set('spark.task.maxFailures', '4')` |
| Memory Pressure | `spark.conf.set('spark.executor.memory', '8g')` and `spark.conf.set('spark.executor.memoryOverhead', '2g')` |
| Executor Hotspot | Check data locality and partition assignment |
| Too Many Partitions | `df.coalesce(N)` to target ~128 MB/partition |
| Too Few Partitions | `df.repartition(N)` to target ~128 MB/partition |
| Broadcast Join Opportunity | `from pyspark.sql.functions import broadcast; df.join(broadcast(small_df), on='key')` |
| Python UDF | Replace `@udf` with `@pandas_udf` for vectorized execution, or use native `F.when()`/`F.expr()` |
| Cache Opportunity | `df.cache()` or `df.persist(StorageLevel.MEMORY_AND_DISK)` before the first action |
| Slow Stage + Large Scan | `df.filter(F.col('date') >= '2024-01-01')` and select only needed columns. Use partition pruning |
| Slow Stage + Wide Shuffle | `from pyspark.sql.functions import broadcast; df.join(broadcast(small_df), ...)`. Pre-aggregate with `groupBy` before joins |
| Slow Stage + Data Explosion | Filter before explode: `df.filter(...).withColumn('x', explode('arr'))` |
| Slow Stage (no pattern) | `df.explain(True)` to see the query plan. Large shuffle may indicate missing filters or broad joins |

## Estimated Savings

Each suspect includes an `estimated_savings_ms` field — a rough estimate of how much time could be saved by addressing the issue. This is used as a secondary sort key (after severity) so that higher-impact issues appear first within the same severity level.

### How savings are computed per category

| Category | Estimation Method |
|----------|-------------------|
| Slow Stage | `executor_run_time - mean_runtime` (time above average) |
| Disk Spill | ~30% of `executor_run_time` (spill overhead) |
| CPU Bottleneck | ~20% of `executor_run_time` |
| I/O Bottleneck | ~20% of `executor_run_time` |
| Record Explosion | ~50% of `executor_run_time` |
| Task Failures | `executor_run_time × failure_rate` (retry overhead) |
| Memory Pressure | ~10% of `executor_run_time` (GC pause overhead) |
| Too Many Partitions | ~40% of `executor_run_time` (scheduling overhead) |
| Too Few Partitions | ~50% of `executor_run_time` (straggler overhead) |
| Broadcast Join Opportunity | ~60% of `executor_run_time` (shuffle elimination) |
| Python UDF | ~50% of `executor_run_time` (serialization overhead) |
| Cache Opportunity | `total_runtime - min_single_runtime` (repeated computation) |

These are heuristic estimates intended for prioritization, not precise predictions.

## SQL Correlation

Each suspect is linked to its originating SQL execution when possible. The suspect shows:

- **SQL ID** — the Spark SQL execution identifier
- **SQL Description** — the query text or description
- **SQL Plan Hint** — the top operations from the physical plan (e.g., "HashAggregate -> Exchange -> Scan parquet")

This helps trace the suspect back to the specific query that caused it.

## I/O Summary

Slow stage and spill suspects include an I/O summary showing:

- Input bytes / records
- Output bytes / records
- Shuffle read bytes / records
- Shuffle write bytes / records
- Memory and disk spill amounts

Use this to understand the data flow through the flagged stage.

## Severity Sorting

Suspects are sorted by severity (Critical first, then Warning), with `estimated_savings_ms` descending as a tiebreaker within the same severity level. The Suspects tab title reflects this: `"Suspects (severity → savings)"`.

## Color-Coding in Stage Detail

The stage detail view uses color-coded metrics to help identify issues at a glance.

### CPU Utilization

The CPU % value in the stage header is color-coded based on the CPU ratio (`executor_cpu_time / executor_run_time`):

| Color | Range | Meaning |
|-------|-------|---------|
| Red | ≥ 95% | CPU saturated |
| Green | 50%–94% | Healthy utilization |
| Yellow | 30%–49% | Underutilized (possible I/O bound) |
| Red | < 30% | Severe I/O bound |

### Peak Memory

Peak execution memory is color-coded relative to total cluster memory when executor data is available:

| Color | Ratio to cluster memory | Meaning |
|-------|------------------------|---------|
| Red | ≥ 80% | Near memory limit |
| Yellow | 50%–79% | Moderate usage |
| Green | 10%–49% | Comfortable |
| Default | < 10% | Low usage |

When executor data is unavailable, absolute thresholds are used as fallback:

| Color | Threshold | Meaning |
|-------|-----------|---------|
| Red | ≥ 10 GB | High memory usage |
| Yellow | ≥ 1 GB | Moderate |
| Green | ≥ 100 MB | Normal |
| Default | < 100 MB | Low |
