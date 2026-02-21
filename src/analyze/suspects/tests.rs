use std::collections::HashMap;

use super::bottleneck::classify_bottleneck;
use super::*;
use crate::analyze::types::*;
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
