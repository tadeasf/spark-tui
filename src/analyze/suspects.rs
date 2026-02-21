use std::collections::HashMap;

use super::types::{BottleneckPattern, Severity, Suspect, SuspectCategory};
use crate::fetch::types::{SparkStage, StageStatus};
use crate::util::format::{format_bytes, format_duration_ms, parse_plan_top_operations};

const ONE_HUNDRED_MB: i64 = 104_857_600;
const FIVE_HUNDRED_MB: i64 = 524_288_000;
const ONE_GB: i64 = 1_073_741_824;

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

/// Get a pattern-specific recommendation for a bottleneck.
fn bottleneck_recommendation(pattern: BottleneckPattern) -> &'static str {
    match pattern {
        BottleneckPattern::LargeScan => {
            "Add partition filters or push down predicates to reduce scan volume."
        }
        BottleneckPattern::WideShuffle => {
            "Reduce shuffle: broadcast join small tables, pre-aggregate, or repartition()."
        }
        BottleneckPattern::DataExplosion => {
            "Check for cross joins, explode(), cartesian products. Add join conditions."
        }
    }
}

/// Resolve the SQL plan hint for a stage given its sql_id and available plans.
fn resolve_plan_hint(
    stage_id: i64,
    stage_to_job: &HashMap<i64, i64>,
    job_to_sql: &HashMap<i64, i64>,
    sql_plans: &HashMap<i64, String>,
) -> Option<String> {
    let job_id = stage_to_job.get(&stage_id).copied()?;
    let sql_id = job_to_sql.get(&job_id).copied()?;
    let plan = sql_plans.get(&sql_id)?;
    let ops = parse_plan_top_operations(plan, 3);
    if ops.is_empty() {
        return None;
    }
    Some(ops.join(" → "))
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
    // Add in:out ratio when both > 0 and ratio > 2.0
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

/// Look up the SQL id and description for a stage via its job.
fn resolve_sql(
    stage_id: i64,
    stage_to_job: &HashMap<i64, i64>,
    job_to_sql: &HashMap<i64, i64>,
    sql_descriptions: &HashMap<i64, String>,
) -> (Option<i64>, Option<String>) {
    let job_id = stage_to_job.get(&stage_id).copied();
    let sql_id = job_id.and_then(|jid| job_to_sql.get(&jid).copied());
    let sql_desc = sql_id.and_then(|sid| sql_descriptions.get(&sid).cloned());
    (sql_id, sql_desc)
}

/// Detect stages that are significantly slower than average.
/// Flag stages with executor_run_time > 2 stddev above the mean.
pub fn detect_slow_stages(
    stages: &[SparkStage],
    stage_to_job: &HashMap<i64, i64>,
    job_to_sql: &HashMap<i64, i64>,
    sql_descriptions: &HashMap<i64, String>,
    sql_plans: &HashMap<i64, String>,
) -> Vec<Suspect> {
    let completed: Vec<&SparkStage> = stages
        .iter()
        .filter(|s| s.status == StageStatus::Complete && s.executor_run_time > 0)
        .collect();

    if completed.len() < 2 {
        return vec![];
    }

    let n = completed.len() as f64;
    let mean = completed.iter().map(|s| s.executor_run_time as f64).sum::<f64>() / n;
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
            let (sql_id, sql_description) =
                resolve_sql(s.stage_id, stage_to_job, job_to_sql, sql_descriptions);
            let bottleneck = classify_bottleneck(s);
            let tag = bottleneck
                .map(|b| format!(" [{}]", b))
                .unwrap_or_default();
            let mut suspect = Suspect::new(
                severity,
                SuspectCategory::SlowStage,
                s.stage_id,
                stage_to_job.get(&s.stage_id).copied(),
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
            suspect.stage_name = Some(s.name.clone());
            suspect.sql_id = sql_id;
            suspect.sql_description = sql_description;
            suspect.io_summary = Some(stage_io_summary(s));
            suspect.bottleneck = bottleneck;
            suspect.recommendation = Some(
                bottleneck
                    .map(|b| bottleneck_recommendation(b).to_string())
                    .unwrap_or_else(|| {
                        "Check code location. Large shuffle may indicate missing filters or broad joins."
                            .to_string()
                    }),
            );
            suspect.sql_plan_hint =
                resolve_plan_hint(s.stage_id, stage_to_job, job_to_sql, sql_plans);
            suspect
        })
        .collect()
}

/// Detect stages with disk spill. Critical if > 1GB.
pub fn detect_spill(
    stages: &[SparkStage],
    stage_to_job: &HashMap<i64, i64>,
    job_to_sql: &HashMap<i64, i64>,
    sql_descriptions: &HashMap<i64, String>,
    sql_plans: &HashMap<i64, String>,
) -> Vec<Suspect> {
    stages
        .iter()
        .filter(|s| s.disk_bytes_spilled > 0)
        .map(|s| {
            let severity = if s.disk_bytes_spilled > ONE_GB {
                Severity::Critical
            } else {
                Severity::Warning
            };
            let (sql_id, sql_description) =
                resolve_sql(s.stage_id, stage_to_job, job_to_sql, sql_descriptions);
            let bottleneck = classify_bottleneck(s);
            let mut suspect = Suspect::new(
                severity,
                SuspectCategory::DiskSpill,
                s.stage_id,
                stage_to_job.get(&s.stage_id).copied(),
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
            suspect.stage_name = Some(s.name.clone());
            suspect.sql_id = sql_id;
            suspect.sql_description = sql_description;
            suspect.io_summary = Some(stage_io_summary(s));
            suspect.bottleneck = bottleneck;
            suspect.recommendation = Some(match bottleneck {
                Some(b) => format!(
                    "Increase spark.executor.memory or reduce partition size. {}",
                    bottleneck_recommendation(b)
                ),
                None => "Increase spark.executor.memory or reduce partition size with repartition()."
                    .to_string(),
            });
            suspect.sql_plan_hint =
                resolve_plan_hint(s.stage_id, stage_to_job, job_to_sql, sql_plans);
            suspect
        })
        .collect()
}

/// Aggregate all suspects and sort by severity (Critical first).
pub fn aggregate_suspects(mut suspects: Vec<Suspect>) -> Vec<Suspect> {
    suspects.sort_by(|a, b| b.severity.cmp(&a.severity));
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
            name: format!("Stage {}", stage_id),
            submission_time: None,
            completion_time: None,
            first_task_launched_time: None,
        }
    }

    #[test]
    fn test_no_slow_stages_uniform() {
        let stages: Vec<SparkStage> = (0..10).map(|i| make_stage(i, 1000, 0)).collect();
        let map = HashMap::new();
        let suspects = detect_slow_stages(&stages, &map, &HashMap::new(), &HashMap::new(), &HashMap::new());
        assert!(suspects.is_empty());
    }

    #[test]
    fn test_slow_stage_detected() {
        let mut stages: Vec<SparkStage> = (0..9).map(|i| make_stage(i, 1000, 0)).collect();
        stages.push(make_stage(9, 100000, 0));
        let map = HashMap::new();
        let suspects = detect_slow_stages(&stages, &map, &HashMap::new(), &HashMap::new(), &HashMap::new());
        assert!(!suspects.is_empty());
        assert_eq!(suspects[0].stage_id, 9);
        assert!(suspects[0].stage_name.is_some());
        assert!(suspects[0].recommendation.is_some());
    }

    #[test]
    fn test_spill_detected() {
        let stages = vec![make_stage(0, 1000, 500_000_000)];
        let map = HashMap::new();
        let suspects = detect_spill(&stages, &map, &HashMap::new(), &HashMap::new(), &HashMap::new());
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0].severity, Severity::Warning);
        assert!(suspects[0].recommendation.is_some());
    }

    #[test]
    fn test_critical_spill() {
        let stages = vec![make_stage(0, 1000, 2_000_000_000)];
        let map = HashMap::new();
        let suspects = detect_spill(&stages, &map, &HashMap::new(), &HashMap::new(), &HashMap::new());
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
        assert_eq!(classify_bottleneck(&s), Some(BottleneckPattern::WideShuffle));
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
}
