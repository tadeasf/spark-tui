use std::collections::HashMap;

use super::types::{Severity, Suspect, SuspectCategory};
use crate::fetch::types::SparkTask;
use crate::util::format::{format_bytes, format_duration_ms};

/// Analyze a distribution of values and return (cv, max_median_ratio, median, max_val).
/// Returns None if there are fewer than 2 values or mean is zero.
fn analyze_distribution(values: &[f64]) -> Option<(f64, f64, f64, f64)> {
    if values.len() < 2 {
        return None;
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    if mean <= 0.0 {
        return None;
    }
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    let stddev = variance.sqrt();
    let cv = stddev / mean;

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = if sorted.len().is_multiple_of(2) {
        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
    } else {
        sorted[sorted.len() / 2]
    };
    let max_val = sorted.last().copied().unwrap_or(0.0);
    let max_median_ratio = if median > 0.0 { max_val / median } else { 0.0 };
    Some((cv, max_median_ratio, median, max_val))
}

/// Check if distribution metrics indicate skew.
fn is_skew(cv: f64, max_median_ratio: f64) -> (bool, bool) {
    let is_critical = cv > 2.0 || max_median_ratio > 10.0;
    let is_warning = cv > 1.0 || max_median_ratio > 3.0;
    (is_warning, is_critical)
}

/// Detect all forms of skew in a list of tasks for a given stage.
/// Returns a Vec of Suspects (duration skew, data-size skew, record-count skew, executor hotspot).
pub fn detect_skew(
    tasks: &[SparkTask],
    stage_id: i64,
    job_id: Option<i64>,
    stage_name: Option<&str>,
    sql_id: Option<i64>,
    sql_description: Option<&str>,
) -> Vec<Suspect> {
    let mut suspects = Vec::new();

    // -- Duration skew --
    let durations: Vec<f64> = tasks
        .iter()
        .filter_map(|t| t.duration.map(|d| d as f64))
        .collect();

    if let Some((cv, max_median_ratio, median, _max_val)) = analyze_distribution(&durations) {
        let (is_warning, is_critical) = is_skew(cv, max_median_ratio);
        if is_warning {
            let severity = if is_critical {
                Severity::Critical
            } else {
                Severity::Warning
            };
            let max_task = tasks
                .iter()
                .max_by_key(|t| t.duration.unwrap_or(0))
                .unwrap();
            let max_bytes = max_task.input_bytes + max_task.shuffle_read_bytes;

            let mut suspect = Suspect::new(
                severity,
                SuspectCategory::DataSkew,
                stage_id,
                job_id,
                format!(
                    "Duration skew: CV={:.1}, max/median={:.1}x",
                    cv, max_median_ratio
                ),
                format!(
                    "Task #{} took {} (median {}), processed {}",
                    max_task.task_id,
                    format_duration_ms(max_task.duration.unwrap_or(0)),
                    format_duration_ms(median as i64),
                    format_bytes(max_bytes),
                ),
            );
            suspect.stage_name = stage_name.map(|s| s.to_string());
            suspect.sql_id = sql_id;
            suspect.sql_description = sql_description.map(|s| s.to_string());
            suspect.recommendation =
                Some("Consider repartitioning or salting skewed keys.".to_string());
            suspects.push(suspect);
        }
    }

    // -- Data size skew --
    let data_sizes: Vec<f64> = tasks
        .iter()
        .map(|t| (t.input_bytes + t.shuffle_read_bytes) as f64)
        .collect();

    if let Some((cv, max_median_ratio, median, max_val)) = analyze_distribution(&data_sizes) {
        let (is_warning, is_critical) = is_skew(cv, max_median_ratio);
        if is_warning {
            let severity = if is_critical {
                Severity::Critical
            } else {
                Severity::Warning
            };
            let max_task = tasks
                .iter()
                .max_by_key(|t| t.input_bytes + t.shuffle_read_bytes)
                .unwrap();

            let mut suspect = Suspect::new(
                severity,
                SuspectCategory::DataSizeSkew,
                stage_id,
                job_id,
                format!(
                    "Data size skew: CV={:.1}, max/median={:.1}x",
                    cv, max_median_ratio
                ),
                format!(
                    "Task #{} processed {} (median {}, max {})",
                    max_task.task_id,
                    format_bytes(max_task.input_bytes + max_task.shuffle_read_bytes),
                    format_bytes(median as i64),
                    format_bytes(max_val as i64),
                ),
            );
            suspect.stage_name = stage_name.map(|s| s.to_string());
            suspect.sql_id = sql_id;
            suspect.sql_description = sql_description.map(|s| s.to_string());
            suspect.recommendation = Some(
                "Data is unevenly distributed. Repartition by a more uniform key or use salting."
                    .to_string(),
            );
            suspects.push(suspect);
        }
    }

    // -- Record count skew --
    let record_counts: Vec<f64> = tasks
        .iter()
        .map(|t| (t.input_records + t.shuffle_read_records) as f64)
        .collect();

    if let Some((cv, max_median_ratio, median, max_val)) = analyze_distribution(&record_counts) {
        let (is_warning, is_critical) = is_skew(cv, max_median_ratio);
        if is_warning && max_val > 1000.0 {
            let severity = if is_critical {
                Severity::Critical
            } else {
                Severity::Warning
            };
            let max_task = tasks
                .iter()
                .max_by_key(|t| t.input_records + t.shuffle_read_records)
                .unwrap();

            let mut suspect = Suspect::new(
                severity,
                SuspectCategory::RecordCountSkew,
                stage_id,
                job_id,
                format!(
                    "Record count skew: CV={:.1}, max/median={:.1}x",
                    cv, max_median_ratio
                ),
                format!(
                    "Task #{} processed {} records (median {:.0}, max {:.0})",
                    max_task.task_id,
                    max_task.input_records + max_task.shuffle_read_records,
                    median,
                    max_val,
                ),
            );
            suspect.stage_name = stage_name.map(|s| s.to_string());
            suspect.sql_id = sql_id;
            suspect.sql_description = sql_description.map(|s| s.to_string());
            suspect.recommendation = Some(
                "Record count is highly skewed. Check for hot keys in joins or group-bys."
                    .to_string(),
            );
            suspects.push(suspect);
        }
    }

    // -- Executor hotspot detection --
    if tasks.len() >= 2 {
        let mut executor_bytes: HashMap<&str, i64> = HashMap::new();
        let mut executor_tasks: HashMap<&str, i64> = HashMap::new();
        for t in tasks {
            let bytes = t.input_bytes + t.shuffle_read_bytes;
            *executor_bytes.entry(t.executor_id.as_str()).or_default() += bytes;
            *executor_tasks.entry(t.executor_id.as_str()).or_default() += 1;
        }
        let total_bytes: i64 = executor_bytes.values().sum();
        if total_bytes > 0 {
            for (exec_id, &bytes) in &executor_bytes {
                let pct = bytes as f64 / total_bytes as f64;
                if pct > 0.5 {
                    let task_count = executor_tasks.get(exec_id).copied().unwrap_or(0);
                    let mut suspect = Suspect::new(
                        Severity::Warning,
                        SuspectCategory::ExecutorHotspot,
                        stage_id,
                        job_id,
                        format!("Executor {} handles {:.0}% of data", exec_id, pct * 100.0),
                        format!(
                            "Executor {} processed {} ({:.0}% of total {}) across {} tasks",
                            exec_id,
                            format_bytes(bytes),
                            pct * 100.0,
                            format_bytes(total_bytes),
                            task_count,
                        ),
                    );
                    suspect.stage_name = stage_name.map(|s| s.to_string());
                    suspect.sql_id = sql_id;
                    suspect.sql_description = sql_description.map(|s| s.to_string());
                    suspect.recommendation = Some(
                        "One executor is processing most data. Check data locality and partition assignment."
                            .to_string(),
                    );
                    suspects.push(suspect);
                }
            }
        }
    }

    suspects
}

#[cfg(test)]
mod tests;
