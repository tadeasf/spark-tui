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
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
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
mod tests {
    use super::*;

    fn make_task(task_id: i64, stage_id: i64, duration: i64) -> SparkTask {
        SparkTask {
            task_id,
            index: 0,
            attempt: 0,
            stage_id,
            stage_attempt_id: 0,
            executor_id: "0".to_string(),
            host: "host".to_string(),
            status: "SUCCESS".to_string(),
            duration: Some(duration),
            executor_run_time: duration,
            executor_cpu_time: 0,
            input_bytes: 1000,
            input_records: 10,
            output_bytes: 0,
            output_records: 0,
            shuffle_read_bytes: 0,
            shuffle_read_records: 0,
            shuffle_write_bytes: 0,
            shuffle_write_records: 0,
            memory_bytes_spilled: 0,
            disk_bytes_spilled: 0,
            peak_execution_memory: 0,
        }
    }

    fn make_task_with_data(
        task_id: i64,
        stage_id: i64,
        duration: i64,
        input_bytes: i64,
        input_records: i64,
        executor_id: &str,
    ) -> SparkTask {
        SparkTask {
            task_id,
            index: 0,
            attempt: 0,
            stage_id,
            stage_attempt_id: 0,
            executor_id: executor_id.to_string(),
            host: "host".to_string(),
            status: "SUCCESS".to_string(),
            duration: Some(duration),
            executor_run_time: duration,
            executor_cpu_time: 0,
            input_bytes,
            input_records,
            output_bytes: 0,
            output_records: 0,
            shuffle_read_bytes: 0,
            shuffle_read_records: 0,
            shuffle_write_bytes: 0,
            shuffle_write_records: 0,
            memory_bytes_spilled: 0,
            disk_bytes_spilled: 0,
            peak_execution_memory: 0,
        }
    }

    #[test]
    fn test_no_skew_uniform_tasks() {
        let tasks: Vec<SparkTask> = (0..100).map(|i| make_task(i, 1, 1000)).collect();
        let suspects = detect_skew(&tasks, 1, Some(0), None, None, None);
        // Duration skew should not be detected; data/record skew won't fire either (uniform)
        assert!(
            !suspects
                .iter()
                .any(|s| s.category == SuspectCategory::DataSkew)
        );
    }

    #[test]
    fn test_skew_detected() {
        let mut tasks: Vec<SparkTask> = (0..99).map(|i| make_task(i, 1, 1000)).collect();
        tasks.push(make_task(99, 1, 100000));
        let suspects = detect_skew(&tasks, 1, Some(0), None, None, None);
        assert!(
            suspects
                .iter()
                .any(|s| s.category == SuspectCategory::DataSkew)
        );
    }

    #[test]
    fn test_critical_skew() {
        let mut tasks: Vec<SparkTask> = (0..99).map(|i| make_task(i, 1, 100)).collect();
        tasks.push(make_task(99, 1, 500000));
        let suspects = detect_skew(&tasks, 1, Some(0), None, None, None);
        let skew = suspects
            .iter()
            .find(|s| s.category == SuspectCategory::DataSkew)
            .unwrap();
        assert_eq!(skew.severity, Severity::Critical);
    }

    #[test]
    fn test_data_size_skew() {
        let mut tasks: Vec<SparkTask> = (0..99)
            .map(|i| make_task_with_data(i, 1, 1000, 100, 10, "exec-0"))
            .collect();
        // One task processes way more data
        tasks.push(make_task_with_data(99, 1, 1000, 100_000, 10, "exec-0"));
        let suspects = detect_skew(&tasks, 1, Some(0), None, None, None);
        assert!(
            suspects
                .iter()
                .any(|s| s.category == SuspectCategory::DataSizeSkew)
        );
    }

    #[test]
    fn test_record_count_skew() {
        let mut tasks: Vec<SparkTask> = (0..99)
            .map(|i| make_task_with_data(i, 1, 1000, 100, 100, "exec-0"))
            .collect();
        // One task processes way more records
        tasks.push(make_task_with_data(99, 1, 1000, 100, 100_000, "exec-0"));
        let suspects = detect_skew(&tasks, 1, Some(0), None, None, None);
        assert!(
            suspects
                .iter()
                .any(|s| s.category == SuspectCategory::RecordCountSkew)
        );
    }

    #[test]
    fn test_executor_hotspot() {
        let mut tasks: Vec<SparkTask> = (0..10)
            .map(|i| make_task_with_data(i, 1, 1000, 100, 10, "exec-0"))
            .collect();
        // exec-1 handles the vast majority of data
        for i in 10..20 {
            tasks.push(make_task_with_data(i, 1, 1000, 10_000, 10, "exec-1"));
        }
        let suspects = detect_skew(&tasks, 1, Some(0), None, None, None);
        assert!(
            suspects
                .iter()
                .any(|s| s.category == SuspectCategory::ExecutorHotspot)
        );
    }

    #[test]
    fn test_analyze_distribution_uniform() {
        let values: Vec<f64> = (0..100).map(|_| 1000.0).collect();
        let result = analyze_distribution(&values);
        assert!(result.is_some());
        let (cv, max_median_ratio, _, _) = result.unwrap();
        assert!(cv < 0.01);
        assert!((max_median_ratio - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_analyze_distribution_skewed() {
        let mut values: Vec<f64> = (0..99).map(|_| 100.0).collect();
        values.push(10_000.0);
        let result = analyze_distribution(&values);
        assert!(result.is_some());
        let (cv, max_median_ratio, _, _) = result.unwrap();
        assert!(cv > 1.0);
        assert!(max_median_ratio > 3.0);
    }
}
