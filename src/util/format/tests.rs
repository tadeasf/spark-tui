use super::*;

#[test]
fn test_format_duration_ms() {
    assert_eq!(format_duration_ms(0), "0ms");
    assert_eq!(format_duration_ms(500), "500ms");
    assert_eq!(format_duration_ms(999), "999ms");
    assert_eq!(format_duration_ms(1000), "1s");
    assert_eq!(format_duration_ms(1500), "1.5s");
    assert_eq!(format_duration_ms(60000), "1m 0s");
    assert_eq!(format_duration_ms(90000), "1m 30s");
    assert_eq!(format_duration_ms(3600000), "1h 0m");
    assert_eq!(format_duration_ms(3660000), "1h 1m");
    assert_eq!(format_duration_ms(-1), "N/A");
}

#[test]
fn test_format_bytes() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(1024), "1.0 KB");
    assert_eq!(format_bytes(1536), "1.5 KB");
    assert_eq!(format_bytes(1048576), "1.0 MB");
    assert_eq!(format_bytes(1073741824), "1.0 GB");
    assert_eq!(format_bytes(1099511627776), "1.0 TB");
    assert_eq!(format_bytes(-1), "N/A");
}

#[test]
fn test_truncate() {
    assert_eq!(truncate("hello", 10), "hello");
    assert_eq!(truncate("hello world", 8), "hello...");
    assert_eq!(truncate("hi", 2), "hi");
    assert_eq!(truncate("hello", 3), "hel");
}

#[test]
fn test_clean_stage_name_plain() {
    assert_eq!(
        clean_stage_name("aggregate at MyFile.scala:42"),
        "aggregate at MyFile.scala:42"
    );
}

#[test]
fn test_clean_stage_name_spark_connect() {
    let name = "Spark Connect - session_id: \"abc-def-123\" - collect at script.py:10";
    assert_eq!(clean_stage_name(name), "collect at script.py:10");
}

#[test]
fn test_clean_stage_name_uuid_only() {
    let name = "Spark Connect - session_id: no-suffix";
    assert_eq!(clean_stage_name(name), name);
}

#[test]
fn test_parse_plan_top_operations() {
    let plan = r#"== Physical Plan ==
AdaptiveSparkPlan (65)
+- Exchange hashpartitioning(col#123, 200)
   +- Filter (isnotnull(col#456))
      +- Scan parquet db.table [col1, col2]"#;
    let ops = parse_plan_top_operations(plan, 5);
    assert_eq!(
        ops,
        vec![
            "AdaptiveSparkPlan",
            "Exchange hashpartitioning",
            "Filter",
            "Scan parquet db.table"
        ]
    );
}

#[test]
fn test_parse_plan_top_operations_limit() {
    let plan = "Sort (1)\n+- Filter (2)\n   +- Scan (3)\n";
    let ops = parse_plan_top_operations(plan, 2);
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0], "Sort");
    assert_eq!(ops[1], "Filter");
}

#[test]
fn test_parse_plan_empty() {
    assert!(parse_plan_top_operations("", 3).is_empty());
}

#[test]
fn test_format_records() {
    assert_eq!(format_records(0), "0");
    assert_eq!(format_records(999), "999");
    assert_eq!(format_records(1500), "1.5K");
    assert_eq!(format_records(1_500_000), "1.5M");
    assert_eq!(format_records(2_500_000_000), "2.5B");
    assert_eq!(format_records(-1), "N/A");
}

#[test]
fn test_percentile_basic() {
    let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
    assert!((percentile(&values, 0.0) - 1.0).abs() < 0.01);
    assert!((percentile(&values, 0.5) - 3.0).abs() < 0.01);
    assert!((percentile(&values, 1.0) - 5.0).abs() < 0.01);
}

#[test]
fn test_percentile_empty() {
    assert_eq!(percentile(&[], 0.5), 0.0);
}

#[test]
fn test_percentile_single() {
    assert_eq!(percentile(&[42.0], 0.5), 42.0);
}

#[test]
fn test_percentile_p90() {
    let values: Vec<f64> = (1..=100).map(|i| i as f64).collect();
    let p90 = percentile(&values, 0.9);
    assert!((p90 - 90.1).abs() < 0.5); // approximately 90
}

#[test]
fn test_sanitize_for_span() {
    assert_eq!(sanitize_for_span("hello\nworld"), "hello world");
    assert_eq!(sanitize_for_span("foo\r\nbar"), "foo  bar");
    assert_eq!(sanitize_for_span("tab\there"), "tab here");
    assert_eq!(sanitize_for_span("clean"), "clean");
}
