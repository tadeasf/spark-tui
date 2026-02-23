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
