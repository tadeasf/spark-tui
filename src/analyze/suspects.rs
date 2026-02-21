use std::collections::HashMap;

use super::types::{BottleneckPattern, Severity, Suspect, SuspectCategory};
use crate::fetch::types::{SparkStage, StageStatus};
use crate::util::format::{format_bytes, format_duration_ms, parse_plan_top_operations};

const ONE_MB: i64 = 1_048_576;
const FIFTY_MB: i64 = 50 * ONE_MB;
const ONE_HUNDRED_MB: i64 = 100 * ONE_MB;
const FIVE_HUNDRED_MB: i64 = 500 * ONE_MB;
const ONE_GB: i64 = 1024 * ONE_MB;

/// Holds all lookup maps needed by suspect detectors, eliminating repetitive parameters.
pub struct SuspectContext<'a> {
    pub stage_to_job: &'a HashMap<i64, i64>,
    pub job_to_sql: &'a HashMap<i64, i64>,
    pub sql_descriptions: &'a HashMap<i64, String>,
    pub sql_plans: &'a HashMap<i64, String>,
}

impl<'a> SuspectContext<'a> {
    pub fn new(
        stage_to_job: &'a HashMap<i64, i64>,
        job_to_sql: &'a HashMap<i64, i64>,
        sql_descriptions: &'a HashMap<i64, String>,
        sql_plans: &'a HashMap<i64, String>,
    ) -> Self {
        Self {
            stage_to_job,
            job_to_sql,
            sql_descriptions,
            sql_plans,
        }
    }

    /// Look up the job_id for a stage.
    pub fn job_id(&self, stage_id: i64) -> Option<i64> {
        self.stage_to_job.get(&stage_id).copied()
    }

    /// Resolve the SQL id and description for a stage via its job.
    fn resolve_sql(&self, stage_id: i64) -> (Option<i64>, Option<String>) {
        let job_id = self.stage_to_job.get(&stage_id).copied();
        let sql_id = job_id.and_then(|jid| self.job_to_sql.get(&jid).copied());
        let sql_desc = sql_id.and_then(|sid| self.sql_descriptions.get(&sid).cloned());
        (sql_id, sql_desc)
    }

    /// Resolve the SQL plan hint for a stage.
    pub fn resolve_plan_hint_for(&self, stage_id: i64) -> Option<String> {
        let job_id = self.stage_to_job.get(&stage_id).copied()?;
        let sql_id = self.job_to_sql.get(&job_id).copied()?;
        let plan = self.sql_plans.get(&sql_id)?;
        let ops = parse_plan_top_operations(plan, 3);
        if ops.is_empty() {
            return None;
        }
        Some(ops.join(" → "))
    }

    /// Enrich a suspect with common stage metadata: stage_name, sql linkage, I/O summary, plan hint.
    pub fn enrich(&self, suspect: &mut Suspect, stage: &SparkStage) {
        let (sql_id, sql_description) = self.resolve_sql(stage.stage_id);
        suspect.stage_name = Some(stage.name.clone());
        suspect.sql_id = sql_id;
        suspect.sql_description = sql_description;
        suspect.io_summary = Some(stage_io_summary(stage));
        suspect.sql_plan_hint = self.resolve_plan_hint_for(stage.stage_id);
    }
}

/// Classify the root-cause bottleneck pattern for a stage.
pub fn classify_bottleneck(s: &SparkStage) -> Option<BottleneckPattern> {
    // DataExplosion: input > 100MB && output > 5 * input
    if s.input_bytes > ONE_HUNDRED_MB && s.output_bytes > 5 * s.input_bytes {
        return Some(BottleneckPattern::DataExplosion);
    }
    // LargeScan: input > 1GB && input > 10 * (output + shuffle_write)
    if s.input_bytes > ONE_GB && s.input_bytes > 10 * (s.output_bytes + s.shuffle_write_bytes) {
        return Some(BottleneckPattern::LargeScan);
    }
    // WideShuffle: shuffle_write > 500MB || (shuffle_read > input && input > 0)
    if s.shuffle_write_bytes > FIVE_HUNDRED_MB
        || (s.shuffle_read_bytes > s.input_bytes && s.input_bytes > 0)
    {
        return Some(BottleneckPattern::WideShuffle);
    }
    None
}

/// Get a PySpark-specific recommendation for a bottleneck.
fn bottleneck_recommendation(pattern: BottleneckPattern) -> &'static str {
    match pattern {
        BottleneckPattern::LargeScan => {
            "Filter early: df.filter(F.col('date') >= '2024-01-01') and select only needed columns: df.select('col1', 'col2'). Use partition pruning on date/region columns."
        }
        BottleneckPattern::WideShuffle => {
            "Use broadcast for small tables: from pyspark.sql.functions import broadcast; df.join(broadcast(small_df), ...). Pre-aggregate with groupBy before joins."
        }
        BottleneckPattern::DataExplosion => {
            "Filter before explode, not after: df.filter(...).withColumn('x', explode('arr')). Check for unintentional cross joins — add explicit join conditions."
        }
        BottleneckPattern::RecordExplosion => {
            "Check for explode()/posexplode() on large arrays, or cross joins. Filter input before explode: df.filter(...).select(explode('col'))."
        }
    }
}

/// Build a one-line I/O summary for a stage, including in:out ratio when significant.
fn stage_io_summary(s: &SparkStage) -> String {
    let mut parts = Vec::new();
    if s.input_bytes > 0 {
        parts.push(format!("in: {}", format_bytes(s.input_bytes)));
    }
    if s.output_bytes > 0 {
        parts.push(format!("out: {}", format_bytes(s.output_bytes)));
    }
    if s.shuffle_read_bytes > 0 {
        parts.push(format!("shuf_r: {}", format_bytes(s.shuffle_read_bytes)));
    }
    if s.shuffle_write_bytes > 0 {
        parts.push(format!("shuf_w: {}", format_bytes(s.shuffle_write_bytes)));
    }
    if s.input_bytes > 0 && s.output_bytes > 0 {
        let ratio = s.input_bytes as f64 / s.output_bytes as f64;
        if ratio > 2.0 {
            parts.push(format!("ratio: {:.0}:1 in:out", ratio));
        }
    }
    if parts.is_empty() {
        "no I/O".to_string()
    } else {
        parts.join(", ")
    }
}

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

/// Aggregate all suspects and sort by severity (Critical first), then by estimated savings descending.
pub fn aggregate_suspects(mut suspects: Vec<Suspect>) -> Vec<Suspect> {
    suspects.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| b.estimated_savings_ms.cmp(&a.estimated_savings_ms))
    });
    suspects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::types::*;

    fn make_stage(stage_id: i64, executor_run_time: i64, disk_spill: i64) -> SparkStage {
        SparkStage {
            status: StageStatus::Complete,
            stage_id,
            attempt_id: 0,
            num_tasks: 10,
            num_active_tasks: 0,
            num_complete_tasks: 10,
            num_failed_tasks: 0,
            num_killed_tasks: 0,
            executor_run_time,
            executor_cpu_time: executor_run_time,
            input_bytes: 1000,
            input_records: 10,
            output_bytes: 1000,
            output_records: 10,
            shuffle_read_bytes: 0,
            shuffle_read_records: 0,
            shuffle_write_bytes: 0,
            shuffle_write_records: 0,
            memory_bytes_spilled: disk_spill,
            disk_bytes_spilled: disk_spill,
            peak_execution_memory: 0,
            name: format!("Stage {}", stage_id),
            submission_time: None,
            completion_time: None,
            first_task_launched_time: None,
        }
    }

    fn empty_ctx() -> SuspectContext<'static> {
        // Leak empty HashMaps for test convenience — tiny, static lifetime
        let s2j: &'static HashMap<i64, i64> = Box::leak(Box::default());
        let j2s: &'static HashMap<i64, i64> = Box::leak(Box::default());
        let desc: &'static HashMap<i64, String> = Box::leak(Box::default());
        let plans: &'static HashMap<i64, String> = Box::leak(Box::default());
        SuspectContext::new(s2j, j2s, desc, plans)
    }

    #[test]
    fn test_no_slow_stages_uniform() {
        let stages: Vec<SparkStage> = (0..10).map(|i| make_stage(i, 1000, 0)).collect();
        let ctx = empty_ctx();
        let suspects = detect_slow_stages(&stages, &ctx);
        assert!(suspects.is_empty());
    }

    #[test]
    fn test_slow_stage_detected() {
        let mut stages: Vec<SparkStage> = (0..9).map(|i| make_stage(i, 1000, 0)).collect();
        stages.push(make_stage(9, 100000, 0));
        let ctx = empty_ctx();
        let suspects = detect_slow_stages(&stages, &ctx);
        assert!(!suspects.is_empty());
        assert_eq!(suspects[0].stage_id, 9);
        assert!(suspects[0].stage_name.is_some());
        assert!(suspects[0].recommendation.is_some());
    }

    #[test]
    fn test_spill_detected() {
        let stages = vec![make_stage(0, 1000, 500_000_000)];
        let ctx = empty_ctx();
        let suspects = detect_spill(&stages, &ctx);
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].severity, Severity::Warning);
        assert!(suspects[0].recommendation.is_some());
    }

    #[test]
    fn test_critical_spill() {
        let stages = vec![make_stage(0, 1000, 2_000_000_000)];
        let ctx = empty_ctx();
        let suspects = detect_spill(&stages, &ctx);
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].severity, Severity::Critical);
    }

    fn make_stage_io(
        stage_id: i64,
        input_bytes: i64,
        output_bytes: i64,
        shuffle_write: i64,
        shuffle_read: i64,
    ) -> SparkStage {
        SparkStage {
            status: StageStatus::Complete,
            stage_id,
            attempt_id: 0,
            num_tasks: 10,
            num_active_tasks: 0,
            num_complete_tasks: 10,
            num_failed_tasks: 0,
            num_killed_tasks: 0,
            executor_run_time: 1000,
            executor_cpu_time: 1000,
            input_bytes,
            input_records: 10,
            output_bytes,
            output_records: 10,
            shuffle_read_bytes: shuffle_read,
            shuffle_read_records: 0,
            shuffle_write_bytes: shuffle_write,
            shuffle_write_records: 0,
            memory_bytes_spilled: 0,
            disk_bytes_spilled: 0,
            peak_execution_memory: 0,
            name: format!("Stage {}", stage_id),
            submission_time: None,
            completion_time: None,
            first_task_launched_time: None,
        }
    }

    #[test]
    fn test_classify_large_scan() {
        // 2GB input, 1MB output, no shuffle → LargeScan
        let s = make_stage_io(0, 2_147_483_648, 1_048_576, 0, 0);
        assert_eq!(classify_bottleneck(&s), Some(BottleneckPattern::LargeScan));
    }

    #[test]
    fn test_classify_wide_shuffle() {
        // 100MB input, 100MB output, 600MB shuffle_write → WideShuffle
        let s = make_stage_io(0, 104_857_600, 104_857_600, 629_145_600, 0);
        assert_eq!(
            classify_bottleneck(&s),
            Some(BottleneckPattern::WideShuffle)
        );
    }

    #[test]
    fn test_classify_data_explosion() {
        // 200MB input, 2GB output → DataExplosion
        let s = make_stage_io(0, 209_715_200, 2_147_483_648, 0, 0);
        assert_eq!(
            classify_bottleneck(&s),
            Some(BottleneckPattern::DataExplosion)
        );
    }

    #[test]
    fn test_classify_none() {
        // Small I/O, no anomaly
        let s = make_stage_io(0, 1000, 1000, 0, 0);
        assert_eq!(classify_bottleneck(&s), None);
    }

    #[test]
    fn test_aggregate_sorts_by_severity() {
        let suspects = vec![
            Suspect::new(
                Severity::Warning,
                SuspectCategory::SlowStage,
                1,
                None,
                "warn".to_string(),
                String::new(),
            ),
            Suspect::new(
                Severity::Critical,
                SuspectCategory::DataSkew,
                2,
                None,
                "crit".to_string(),
                String::new(),
            ),
        ];
        let sorted = aggregate_suspects(suspects);
        assert_eq!(sorted[0].severity, Severity::Critical);
        assert_eq!(sorted[1].severity, Severity::Warning);
    }

    #[test]
    fn test_cpu_efficiency_io_bound() {
        // CPU time is 10% of runtime → I/O bound
        let mut stage = make_stage(0, 30_000, 0);
        stage.executor_cpu_time = 3_000 * 1_000_000; // 3s in ns, runtime is 30s
        let stages = vec![stage];
        let ctx = empty_ctx();
        let suspects = detect_cpu_efficiency(&stages, &ctx);
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].category, SuspectCategory::IoBottleneck);
    }

    #[test]
    fn test_cpu_efficiency_cpu_saturated() {
        // CPU time is 95% of runtime, runtime > 30s → CPU saturated
        let mut stage = make_stage(0, 50_000, 0);
        stage.executor_cpu_time = 48_000 * 1_000_000; // 48s in ns, runtime is 50s
        let stages = vec![stage];
        let ctx = empty_ctx();
        let suspects = detect_cpu_efficiency(&stages, &ctx);
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].category, SuspectCategory::CpuBottleneck);
    }

    #[test]
    fn test_cpu_efficiency_normal() {
        // CPU time is 50% of runtime → normal, no suspect
        let mut stage = make_stage(0, 30_000, 0);
        stage.executor_cpu_time = 15_000 * 1_000_000; // 15s in ns
        let stages = vec![stage];
        let ctx = empty_ctx();
        let suspects = detect_cpu_efficiency(&stages, &ctx);
        assert!(suspects.is_empty());
    }

    #[test]
    fn test_record_explosion_detected() {
        let mut stage = make_stage_io(0, 1_000_000, 1_000_000, 0, 0);
        stage.input_records = 10_000;
        stage.output_records = 200_000; // 20x
        let stages = vec![stage];
        let ctx = empty_ctx();
        let suspects = detect_record_explosion(&stages, &ctx);
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].category, SuspectCategory::RecordExplosion);
    }

    #[test]
    fn test_record_explosion_not_triggered_small_input() {
        let mut stage = make_stage_io(0, 1_000_000, 1_000_000, 0, 0);
        stage.input_records = 5; // too few
        stage.output_records = 500;
        let stages = vec![stage];
        let ctx = empty_ctx();
        let suspects = detect_record_explosion(&stages, &ctx);
        assert!(suspects.is_empty());
    }

    #[test]
    fn test_task_failures_detected() {
        let mut stage = make_stage(0, 1000, 0);
        stage.num_tasks = 100;
        stage.num_failed_tasks = 2; // 2% failure rate → Warning
        let stages = vec![stage];
        let ctx = empty_ctx();
        let suspects = detect_task_failures(&stages, &ctx);
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].category, SuspectCategory::TaskFailures);
        assert_eq!(suspects[0].severity, Severity::Warning);
    }

    #[test]
    fn test_task_failures_critical() {
        let mut stage = make_stage(0, 1000, 0);
        stage.num_failed_tasks = 15; // > 10 → critical
        let stages = vec![stage];
        let ctx = empty_ctx();
        let suspects = detect_task_failures(&stages, &ctx);
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].severity, Severity::Critical);
    }

    #[test]
    fn test_memory_pressure_detected() {
        let mut stage = make_stage(0, 1000, 0);
        stage.memory_bytes_spilled = 100_000_000; // 100 MB
        stage.disk_bytes_spilled = 0;
        let stages = vec![stage];
        let ctx = empty_ctx();
        let suspects = detect_memory_pressure(&stages, &ctx);
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].category, SuspectCategory::MemoryPressure);
    }

    #[test]
    fn test_memory_pressure_not_triggered_with_disk_spill() {
        let mut stage = make_stage(0, 1000, 100_000_000);
        // disk_bytes_spilled > 0, so memory pressure shouldn't fire
        stage.memory_bytes_spilled = 100_000_000;
        let stages = vec![stage];
        let ctx = empty_ctx();
        let suspects = detect_memory_pressure(&stages, &ctx);
        assert!(suspects.is_empty());
    }
}
