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

Each suspect includes a recommendation based on its category and bottleneck pattern:

| Category + Bottleneck | Recommendation |
|-----------------------|----------------|
| Data Skew | Repartition or salt skewed keys |
| Data Size Skew | Repartition by a more uniform key or use salting |
| Record Count Skew | Check for hot keys in joins or group-bys |
| Disk Spill | Increase `spark.executor.memory` or reduce partition size |
| CPU Bottleneck | Cache intermediate results, simplify UDFs, increase parallelism |
| I/O Bottleneck | Increase memory, improve data locality, use faster storage, check GC pauses |
| Record Explosion | Check for `explode()`, cross joins, or `generate()`; filter before expanding |
| Task Failures | Check executor logs; common causes: OOM, data corruption, fetch failures |
| Memory Pressure | Increase `spark.executor.memory` or `spark.executor.memoryOverhead`, reduce partition size |
| Executor Hotspot | Check data locality and partition assignment |
| Slow Stage + Large Scan | Add partition pruning or pushdown filters |
| Slow Stage + Wide Shuffle | Reduce shuffle by filtering earlier or using broadcast joins |
| Slow Stage + Data Explosion | Review `explode` calls or cross joins; filter before expanding |
| Slow Stage (no pattern) | Check code location; large shuffle may indicate missing filters or broad joins |

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

Suspects are sorted by severity (Critical first, then Warning), so the most important issues appear at the top of the Suspects tab.

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
