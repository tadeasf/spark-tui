use super::types::{Severity, Suspect, SuspectCategory};
use crate::fetch::types::SparkTask;
use crate::util::format::{format_bytes, format_duration_ms};

/// Detect data skew in a list of tasks for a given stage.
/// Returns a Suspect if skew is detected, None otherwise.
pub fn detect_skew(
    tasks: &[SparkTask],
    stage_id: i64,
    job_id: Option<i64>,
    stage_name: Option<&str>,
    sql_id: Option<i64>,
    sql_description: Option<&str>,
) -> Option<Suspect> {
    let durations: Vec<f64> = tasks
        .iter()
        .filter_map(|t| t.duration.map(|d| d as f64))
        .collect();

    if durations.len() < 2 {
        return None;
    }

    let n = durations.len() as f64;
    let mean = durations.iter().sum::<f64>() / n;
    if mean <= 0.0 {
        return None;
    }

    let variance = durations.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / n;
    let stddev = variance.sqrt();
    let cv = stddev / mean;

    let mut sorted = durations.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = if sorted.len() % 2 == 0 {
        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
    } else {
        sorted[sorted.len() / 2]
    };
    let max_duration = sorted.last().copied().unwrap_or(0.0);
    let max_median_ratio = if median > 0.0 {
        max_duration / median
    } else {
        0.0
    };

    let is_critical = cv > 2.0 || max_median_ratio > 10.0;
    let is_warning = cv > 1.0 || max_median_ratio > 3.0;

    if !is_warning {
        return None;
    }

    let severity = if is_critical {
        Severity::Critical
    } else {
        Severity::Warning
    };

    // Find the max-duration task for detail
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
            "Task skew: CV={:.1}, max/median={:.1}x",
            cv, max_median_ratio
        ),
        format!(
            "Max task {} took {} (median {}), processed {}",
            max_task.task_id,
            format_duration_ms(max_task.duration.unwrap_or(0)),
            format_duration_ms(median as i64),
            format_bytes(max_bytes),
        ),
    );
    suspect.stage_name = stage_name.map(|s| s.to_string());
    suspect.sql_id = sql_id;
    suspect.sql_description = sql_description.map(|s| s.to_string());
    suspect.recommendation = Some("Consider repartitioning or salting skewed keys.".to_string());

    Some(suspect)
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
        }
    }

    #[test]
    fn test_no_skew_uniform_tasks() {
        let tasks: Vec<SparkTask> = (0..100).map(|i| make_task(i, 1, 1000)).collect();
        assert!(detect_skew(&tasks, 1, Some(0), None, None, None).is_none());
    }

    #[test]
    fn test_skew_detected() {
        let mut tasks: Vec<SparkTask> = (0..99).map(|i| make_task(i, 1, 1000)).collect();
        // Add one extremely slow task
        tasks.push(make_task(99, 1, 100000));
        let suspect = detect_skew(&tasks, 1, Some(0), None, None, None);
        assert!(suspect.is_some());
        let s = suspect.unwrap();
        assert_eq!(s.category, SuspectCategory::DataSkew);
        assert!(s.recommendation.is_some());
    }

    #[test]
    fn test_critical_skew() {
        let mut tasks: Vec<SparkTask> = (0..99).map(|i| make_task(i, 1, 100)).collect();
        // max/median ratio will be very high
        tasks.push(make_task(99, 1, 500000));
        let suspect = detect_skew(&tasks, 1, Some(0), None, None, None).unwrap();
        assert_eq!(suspect.severity, Severity::Critical);
    }
}
