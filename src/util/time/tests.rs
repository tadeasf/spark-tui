use super::*;

#[test]
fn test_parse_rfc3339() {
    let ts = "2024-01-15T10:30:00.000+00:00";
    let dt = parse_spark_timestamp(ts).unwrap();
    assert_eq!(
        dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "2024-01-15T10:30:00.000Z"
    );
}

#[test]
fn test_parse_naive() {
    let ts = "2024-01-15T10:30:00.000";
    let dt = parse_spark_timestamp(ts).unwrap();
    assert_eq!(
        dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "2024-01-15T10:30:00.000Z"
    );
}

#[test]
fn test_duration_between() {
    let start = "2024-01-15T10:30:00.000";
    let end = "2024-01-15T10:30:05.500";
    let ms = duration_between(Some(start), Some(end)).unwrap();
    assert_eq!(ms, 5500);
}

#[test]
fn test_duration_between_none() {
    assert!(duration_between(None, Some("2024-01-15T10:30:00.000")).is_none());
    assert!(duration_between(Some("2024-01-15T10:30:00.000"), None).is_none());
}

#[test]
fn test_parse_gmt_suffix() {
    let ts = "2026-02-21T00:34:18.123GMT";
    let dt = parse_spark_timestamp(ts).unwrap();
    assert_eq!(dt.format("%H:%M:%S").to_string(), "00:34:18");
}

#[test]
fn test_duration_between_gmt() {
    let start = "2026-02-21T00:34:18.000GMT";
    let end = "2026-02-21T00:35:20.500GMT";
    let ms = duration_between(Some(start), Some(end)).unwrap();
    assert_eq!(ms, 62500);
}
