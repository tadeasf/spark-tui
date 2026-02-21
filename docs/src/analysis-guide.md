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

**Note:** Task-level analysis is only performed for the top 5 slowest stages to limit API calls.

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

## Bottleneck Classification

When a slow stage or spill suspect is detected, spark-tui classifies the **root cause** based on I/O patterns:

| Pattern | Condition | Meaning |
|---------|-----------|---------|
| **Data Explosion** | `input > 100 MB` and `output > 5x input` | Stage produces far more data than it reads (e.g., `explode`, cross join) |
| **Large Scan** | `input > 1 GB` and `input > 10x (output + shuffle_write)` | Stage reads a lot but produces little (missing pushdown filters) |
| **Wide Shuffle** | `shuffle_write > 500 MB` or `shuffle_read > input` | Stage shuffles more data than it reads directly (broad join, groupBy on high-cardinality key) |

If none of these patterns match, no bottleneck tag is shown.

## Recommendations

Each suspect includes a recommendation based on its category and bottleneck pattern:

| Category + Bottleneck | Recommendation |
|-----------------------|----------------|
| Data Skew | Repartition or salt skewed keys |
| Disk Spill | Increase `spark.executor.memory` or reduce partition size |
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
