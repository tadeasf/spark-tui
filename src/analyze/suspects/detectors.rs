use std::collections::HashMap;

use super::super::types::{BottleneckPattern, Severity, Suspect, SuspectCategory};
use super::bottleneck::{
    FIFTY_MB, ONE_GB, ONE_HUNDRED_MB, ONE_MB, bottleneck_recommendation, classify_bottleneck,
};
use super::context::SuspectContext;
use crate::fetch::types::{SparkStage, StageStatus};
use crate::util::format::{format_bytes, format_duration_ms};

/// Detect stages that are significantly slower than average.
/// Flag stages with executor_run_time > 2 stddev above the mean.
pub fn detect_slow_stages(stages: &[SparkStage], ctx: &SuspectContext) -> Vec<Suspect> {
    let completed: Vec<&SparkStage> = stages
        .iter()
        .filter(|s| s.status == StageStatus::Complete && s.executor_run_time > 0)
        .collect();

    if completed.len() < 2 {
        return vec![];
    }

    let n = completed.len() as f64;
    let mean = completed
        .iter()
        .map(|s| s.executor_run_time as f64)
        .sum::<f64>()
        / n;
    let variance = completed
        .iter()
        .map(|s| (s.executor_run_time as f64 - mean).powi(2))
        .sum::<f64>()
        / n;
    let stddev = variance.sqrt();

    let threshold_warn = mean + 2.0 * stddev;
    let threshold_crit = mean + 4.0 * stddev;

    completed
        .into_iter()
        .filter(|s| s.executor_run_time as f64 > threshold_warn)
        .map(|s| {
            let severity = if (s.executor_run_time as f64) > threshold_crit {
                Severity::Critical
            } else {
                Severity::Warning
            };
            let bottleneck = classify_bottleneck(s);
            let tag = bottleneck
                .map(|b| format!(" [{}]", b))
                .unwrap_or_default();
            let mut suspect = Suspect::new(
                severity,
                SuspectCategory::SlowStage,
                s.stage_id,
                ctx.job_id(s.stage_id),
                format!(
                    "Stage {} took {}{}",
                    s.stage_id,
                    format_duration_ms(s.executor_run_time),
                    tag,
                ),
                format!(
                    "Mean: {}, StdDev: {}, {:.1}x slower than average",
                    format_duration_ms(mean as i64),
                    format_duration_ms(stddev as i64),
                    s.executor_run_time as f64 / mean
                ),
            );
            ctx.enrich(&mut suspect, s);
            suspect.bottleneck = bottleneck;
            suspect.estimated_savings_ms = (s.executor_run_time as f64 - mean) as i64;
            suspect.recommendation = Some(
                bottleneck
                    .map(|b| bottleneck_recommendation(b).to_string())
                    .unwrap_or_else(|| {
                        "Check code location. Try df.explain(True) to see the query plan. Large shuffle may indicate missing filters or broad joins."
                            .to_string()
                    }),
            );
            suspect
        })
        .collect()
}

/// Detect stages with disk spill. Critical if > 1GB.
pub fn detect_spill(stages: &[SparkStage], ctx: &SuspectContext) -> Vec<Suspect> {
    stages
        .iter()
        .filter(|s| s.disk_bytes_spilled > 0)
        .map(|s| {
            let severity = if s.disk_bytes_spilled > ONE_GB {
                Severity::Critical
            } else {
                Severity::Warning
            };
            let bottleneck = classify_bottleneck(s);
            let mut suspect = Suspect::new(
                severity,
                SuspectCategory::DiskSpill,
                s.stage_id,
                ctx.job_id(s.stage_id),
                format!(
                    "Stage {} spilled {} to disk",
                    s.stage_id,
                    format_bytes(s.disk_bytes_spilled)
                ),
                format!(
                    "Memory spill: {}, Disk spill: {}",
                    format_bytes(s.memory_bytes_spilled),
                    format_bytes(s.disk_bytes_spilled)
                ),
            );
            ctx.enrich(&mut suspect, s);
            suspect.bottleneck = bottleneck;
            // Spill overhead estimated at ~30% of runtime
            suspect.estimated_savings_ms = (s.executor_run_time as f64 * 0.3) as i64;
            suspect.recommendation = Some(match bottleneck {
                Some(b) => format!(
                    "spark.conf.set('spark.executor.memory', '8g') or df.repartition(200). {}",
                    bottleneck_recommendation(b)
                ),
                None => {
                    "spark.conf.set('spark.executor.memory', '8g') or df.repartition(200) to reduce partition size."
                        .to_string()
                }
            });
            suspect
        })
        .collect()
}

/// Detect CPU efficiency issues.
/// cpu_ratio = (executor_cpu_time / 1_000_000) / executor_run_time
/// Low ratio → I/O bound; High ratio with long runtime → CPU saturated.
pub fn detect_cpu_efficiency(stages: &[SparkStage], ctx: &SuspectContext) -> Vec<Suspect> {
    stages
        .iter()
        .filter(|s| s.status == StageStatus::Complete && s.executor_run_time > 10_000)
        .filter_map(|s| {
            let cpu_ms = s.executor_cpu_time / 1_000_000; // ns → ms
            let run_ms = s.executor_run_time;
            if run_ms == 0 {
                return None;
            }
            let ratio = cpu_ms as f64 / run_ms as f64;

            let (severity, category, title, detail, recommendation) =
                if ratio < 0.3 {
                    (
                        Severity::Warning,
                        SuspectCategory::IoBottleneck,
                        format!(
                            "Stage {} I/O bound: CPU ratio {:.0}%",
                            s.stage_id,
                            ratio * 100.0
                        ),
                        format!(
                            "CPU time {} vs runtime {} ({:.0}% utilization)",
                            format_duration_ms(cpu_ms),
                            format_duration_ms(run_ms),
                            ratio * 100.0
                        ),
                        "I/O bound — try: spark.conf.set('spark.executor.memory', '8g'), cache hot DataFrames with df.cache(), or use df.repartition() for better locality."
                            .to_string(),
                    )
                } else if ratio > 0.9 && run_ms > 30_000 {
                    (
                        Severity::Warning,
                        SuspectCategory::CpuBottleneck,
                        format!(
                            "Stage {} CPU saturated: {:.0}% utilization for {}",
                            s.stage_id,
                            ratio * 100.0,
                            format_duration_ms(run_ms)
                        ),
                        format!(
                            "CPU time {} vs runtime {} — CPU fully utilized",
                            format_duration_ms(cpu_ms),
                            format_duration_ms(run_ms)
                        ),
                        "CPU saturated — replace @udf with @pandas_udf or native F.when()/F.expr(). Cache with df.cache() and increase parallelism with df.repartition(N)."
                            .to_string(),
                    )
                } else {
                    return None;
                };

            let mut suspect = Suspect::new(
                severity,
                category,
                s.stage_id,
                ctx.job_id(s.stage_id),
                title,
                detail,
            );
            ctx.enrich(&mut suspect, s);
            suspect.recommendation = Some(recommendation);
            // ~20% savings from fixing CPU/IO bottleneck
            suspect.estimated_savings_ms = (s.executor_run_time as f64 * 0.2) as i64;
            Some(suspect)
        })
        .collect()
}

/// Detect record explosion: output_records > 10x input_records.
pub fn detect_record_explosion(stages: &[SparkStage], ctx: &SuspectContext) -> Vec<Suspect> {
    stages
        .iter()
        .filter(|s| s.input_records > 1000 && s.output_records > 10 * s.input_records)
        .map(|s| {
            let ratio = s.output_records as f64 / s.input_records as f64;
            let severity = if ratio > 100.0 {
                Severity::Critical
            } else {
                Severity::Warning
            };
            let mut suspect = Suspect::new(
                severity,
                SuspectCategory::RecordExplosion,
                s.stage_id,
                ctx.job_id(s.stage_id),
                format!(
                    "Stage {} output {:.0}x more records than input",
                    s.stage_id, ratio
                ),
                format!(
                    "Input: {} records → Output: {} records ({:.0}x expansion)",
                    s.input_records, s.output_records, ratio
                ),
            );
            ctx.enrich(&mut suspect, s);
            suspect.bottleneck = Some(BottleneckPattern::RecordExplosion);
            // ~50% savings from fixing record explosion
            suspect.estimated_savings_ms = (s.executor_run_time as f64 * 0.5) as i64;
            suspect.recommendation = Some(
                "Filter before explode: df.filter(...).select(explode('col')). Check for unintentional cross joins — add explicit join conditions."
                    .to_string(),
            );
            suspect
        })
        .collect()
}

/// Detect stages with task failures or killed tasks.
pub fn detect_task_failures(stages: &[SparkStage], ctx: &SuspectContext) -> Vec<Suspect> {
    stages
        .iter()
        .filter(|s| s.num_failed_tasks > 0 || s.num_killed_tasks > 0)
        .map(|s| {
            let total_problematic = s.num_failed_tasks + s.num_killed_tasks;
            let failure_rate = if s.num_tasks > 0 {
                total_problematic as f64 / s.num_tasks as f64
            } else {
                0.0
            };
            let severity = if failure_rate > 0.1 || total_problematic > 10 {
                Severity::Critical
            } else {
                Severity::Warning
            };
            let mut suspect = Suspect::new(
                severity,
                SuspectCategory::TaskFailures,
                s.stage_id,
                ctx.job_id(s.stage_id),
                format!(
                    "Stage {} has {} failed + {} killed tasks ({:.0}%)",
                    s.stage_id,
                    s.num_failed_tasks,
                    s.num_killed_tasks,
                    failure_rate * 100.0
                ),
                format!(
                    "Total tasks: {}, Failed: {}, Killed: {}, Completed: {}",
                    s.num_tasks, s.num_failed_tasks, s.num_killed_tasks, s.num_complete_tasks
                ),
            );
            ctx.enrich(&mut suspect, s);
            // Estimate: failed tasks add ~failure_rate% overhead via retries
            suspect.estimated_savings_ms = (s.executor_run_time as f64 * failure_rate) as i64;
            suspect.recommendation = Some(
                "Check executor logs for OOM/fetch failures. Try: spark.conf.set('spark.executor.memory', '8g') and spark.conf.set('spark.task.maxFailures', '4')."
                    .to_string(),
            );
            suspect
        })
        .collect()
}

/// Detect memory pressure: memory_bytes_spilled > 50MB but disk_bytes_spilled == 0.
/// This is a proactive warning before disk spill happens.
pub fn detect_memory_pressure(stages: &[SparkStage], ctx: &SuspectContext) -> Vec<Suspect> {
    stages
        .iter()
        .filter(|s| s.memory_bytes_spilled > FIFTY_MB && s.disk_bytes_spilled == 0)
        .map(|s| {
            let mut suspect = Suspect::new(
                Severity::Warning,
                SuspectCategory::MemoryPressure,
                s.stage_id,
                ctx.job_id(s.stage_id),
                format!(
                    "Stage {} memory spill {} (no disk spill yet)",
                    s.stage_id,
                    format_bytes(s.memory_bytes_spilled)
                ),
                format!(
                    "Memory spilled: {}, Disk spilled: 0 — approaching disk spill threshold",
                    format_bytes(s.memory_bytes_spilled)
                ),
            );
            ctx.enrich(&mut suspect, s);
            // ~10% overhead from memory pressure (GC pauses)
            suspect.estimated_savings_ms = (s.executor_run_time as f64 * 0.1) as i64;
            suspect.recommendation = Some(
                "spark.conf.set('spark.executor.memory', '8g') and spark.conf.set('spark.executor.memoryOverhead', '2g'). Reduce partition size with df.repartition(N)."
                    .to_string(),
            );
            suspect
        })
        .collect()
}

/// Detect partition count issues: too many small partitions or too few large ones.
pub fn detect_partition_count(stages: &[SparkStage], ctx: &SuspectContext) -> Vec<Suspect> {
    stages
        .iter()
        .filter(|s| s.status == StageStatus::Complete && s.num_tasks > 0)
        .filter_map(|s| {
            let total_bytes = s.input_bytes + s.shuffle_read_bytes;
            if total_bytes == 0 || s.num_tasks == 0 {
                return None;
            }
            let avg_bytes_per_task = total_bytes / s.num_tasks;

            if s.num_tasks > 10_000 && avg_bytes_per_task < ONE_MB {
                // Too many tiny partitions
                let target = (total_bytes / (128 * ONE_MB)).max(1);
                let mut suspect = Suspect::new(
                    Severity::Warning,
                    SuspectCategory::TooManyPartitions,
                    s.stage_id,
                    ctx.job_id(s.stage_id),
                    format!(
                        "Stage {} has {} tasks with only {}/task avg",
                        s.stage_id,
                        s.num_tasks,
                        format_bytes(avg_bytes_per_task)
                    ),
                    format!(
                        "Total data: {}, {} tasks × {} avg",
                        format_bytes(total_bytes),
                        s.num_tasks,
                        format_bytes(avg_bytes_per_task)
                    ),
                );
                ctx.enrich(&mut suspect, s);
                suspect.estimated_savings_ms = (s.executor_run_time as f64 * 0.4) as i64;
                suspect.recommendation = Some(format!(
                    "Too many small partitions. Try: df.coalesce({}) to target ~128MB/partition.",
                    target
                ));
                Some(suspect)
            } else if s.num_tasks <= 8 && avg_bytes_per_task > ONE_GB {
                // Too few large partitions
                let target = (total_bytes / (128 * ONE_MB)).max(8);
                let mut suspect = Suspect::new(
                    Severity::Warning,
                    SuspectCategory::TooFewPartitions,
                    s.stage_id,
                    ctx.job_id(s.stage_id),
                    format!(
                        "Stage {} has only {} tasks with {}/task avg",
                        s.stage_id,
                        s.num_tasks,
                        format_bytes(avg_bytes_per_task)
                    ),
                    format!(
                        "Total data: {}, {} tasks × {} avg",
                        format_bytes(total_bytes),
                        s.num_tasks,
                        format_bytes(avg_bytes_per_task)
                    ),
                );
                ctx.enrich(&mut suspect, s);
                suspect.estimated_savings_ms = (s.executor_run_time as f64 * 0.5) as i64;
                suspect.recommendation = Some(format!(
                    "Too few partitions causing stragglers. Try: df.repartition({}) to target ~128MB/partition.",
                    target
                ));
                Some(suspect)
            } else {
                None
            }
        })
        .collect()
}

/// Detect broadcast join opportunities: SortMergeJoin/ShuffledHashJoin with one small side (<100MB).
pub fn detect_broadcast_join(stages: &[SparkStage], ctx: &SuspectContext) -> Vec<Suspect> {
    stages
        .iter()
        .filter(|s| {
            s.status == StageStatus::Complete
                && s.shuffle_write_bytes > 0
                && s.shuffle_write_bytes < ONE_HUNDRED_MB
                && s.executor_run_time > 5_000
        })
        .filter_map(|s| {
            // Check if the SQL plan contains join indicators
            let plan_hint = ctx.resolve_plan_hint_for(s.stage_id);
            let has_join_indicator = plan_hint
                .as_ref()
                .is_some_and(|h| {
                    h.contains("SortMerge") || h.contains("ShuffledHash") || h.contains("Join")
                });
            if !has_join_indicator {
                return None;
            }

            let mut suspect = Suspect::new(
                Severity::Warning,
                SuspectCategory::BroadcastJoinOpportunity,
                s.stage_id,
                ctx.job_id(s.stage_id),
                format!(
                    "Stage {} shuffles only {} — broadcast join candidate",
                    s.stage_id,
                    format_bytes(s.shuffle_write_bytes)
                ),
                format!(
                    "Shuffle write: {} < 100MB threshold. A broadcast join would eliminate this shuffle.",
                    format_bytes(s.shuffle_write_bytes)
                ),
            );
            ctx.enrich(&mut suspect, s);
            suspect.estimated_savings_ms = (s.executor_run_time as f64 * 0.6) as i64;
            suspect.recommendation = Some(
                "from pyspark.sql.functions import broadcast; df.join(broadcast(small_df), on='key'). Eliminates shuffle for the small side."
                    .to_string(),
            );
            Some(suspect)
        })
        .collect()
}

/// Detect Python UDFs in SQL plans (ArrowEvalPython, BatchEvalPython, PythonUDF, PythonRunner).
pub fn detect_python_udf(stages: &[SparkStage], ctx: &SuspectContext) -> Vec<Suspect> {
    const PYTHON_MARKERS: &[&str] = &[
        "ArrowEvalPython",
        "BatchEvalPython",
        "PythonUDF",
        "PythonRunner",
    ];

    stages
        .iter()
        .filter(|s| s.status == StageStatus::Complete && s.executor_run_time > 5_000)
        .filter_map(|s| {
            let hint = ctx.resolve_plan_hint_for(s.stage_id)?;
            let has_python = PYTHON_MARKERS.iter().any(|m| hint.contains(m));
            if !has_python {
                return None;
            }

            // Check if also CPU-bound → Critical
            let cpu_ms = s.executor_cpu_time / 1_000_000;
            let ratio = if s.executor_run_time > 0 {
                cpu_ms as f64 / s.executor_run_time as f64
            } else {
                0.0
            };
            let severity = if ratio > 0.9 && s.executor_run_time > 30_000 {
                Severity::Critical
            } else {
                Severity::Warning
            };

            let mut suspect = Suspect::new(
                severity,
                SuspectCategory::PythonUdf,
                s.stage_id,
                ctx.job_id(s.stage_id),
                format!(
                    "Stage {} uses Python UDF ({})",
                    s.stage_id,
                    format_duration_ms(s.executor_run_time)
                ),
                format!(
                    "Python UDF detected in SQL plan. CPU utilization: {:.0}%",
                    ratio * 100.0
                ),
            );
            ctx.enrich(&mut suspect, s);
            suspect.estimated_savings_ms = (s.executor_run_time as f64 * 0.5) as i64;
            suspect.recommendation = Some(
                "Replace @udf with @pandas_udf for vectorized execution, or use native F.when()/F.expr() functions. Python UDFs serialize data row-by-row."
                    .to_string(),
            );
            Some(suspect)
        })
        .collect()
}

/// Detect repeated computations that could benefit from caching.
/// Groups completed stages by cleaned name; if >=2 share a name and total runtime > 30s → CacheOpportunity.
pub fn detect_cache_opportunity(stages: &[SparkStage], ctx: &SuspectContext) -> Vec<Suspect> {
    use crate::util::format::clean_stage_name;

    let completed: Vec<&SparkStage> = stages
        .iter()
        .filter(|s| s.status == StageStatus::Complete && s.executor_run_time > 0)
        .collect();

    // Group by cleaned stage name
    let mut name_groups: HashMap<String, Vec<&SparkStage>> = HashMap::new();
    for s in &completed {
        let clean = clean_stage_name(&s.name).to_string();
        name_groups.entry(clean).or_default().push(s);
    }

    let mut suspects = Vec::new();
    for (name, group) in &name_groups {
        if group.len() < 2 {
            continue;
        }
        let total_runtime: i64 = group.iter().map(|s| s.executor_run_time).sum();
        if total_runtime <= 30_000 {
            continue;
        }

        // Report on the slowest stage in the group
        let slowest = group.iter().max_by_key(|s| s.executor_run_time).unwrap();
        let mut suspect = Suspect::new(
            Severity::Warning,
            SuspectCategory::CacheOpportunity,
            slowest.stage_id,
            ctx.job_id(slowest.stage_id),
            format!(
                "'{}' repeated {} times (total: {})",
                crate::util::format::truncate(name, 40),
                group.len(),
                format_duration_ms(total_runtime)
            ),
            format!(
                "{} stages share name '{}'. Total runtime: {}. Caching could eliminate {} re-computations.",
                group.len(),
                crate::util::format::truncate(name, 30),
                format_duration_ms(total_runtime),
                group.len() - 1
            ),
        );
        ctx.enrich(&mut suspect, slowest);
        // Savings = total runtime minus one execution (you still need it once)
        let min_runtime = group.iter().map(|s| s.executor_run_time).min().unwrap_or(0);
        suspect.estimated_savings_ms = total_runtime - min_runtime;
        suspect.recommendation = Some(
            "df.cache() or df.persist(StorageLevel.MEMORY_AND_DISK) before the first action. Unpersist when no longer needed: df.unpersist()."
                .to_string(),
        );
        suspects.push(suspect);
    }

    suspects
}
