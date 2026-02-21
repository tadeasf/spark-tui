use chrono::{DateTime, Utc};

/// Parse a Spark timestamp string (ISO 8601 / RFC 3339 format, or with a space separator).
pub fn parse_spark_timestamp(ts: &str) -> Option<DateTime<Utc>> {
    // Try RFC 3339 first (e.g., "2024-01-15T10:30:00.000+00:00")
    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
        return Some(dt.with_timezone(&Utc));
    }
    // Try with space separator and timezone offset
    if let Ok(dt) = DateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S%.3f%z") {
        return Some(dt.with_timezone(&Utc));
    }
    // Strip "GMT" suffix (Spark/Databricks common format) and parse as naive UTC
    let stripped = ts.strip_suffix("GMT").unwrap_or(ts);
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(stripped, "%Y-%m-%dT%H:%M:%S%.3f") {
        return Some(dt.and_utc());
    }
    // Try naive without milliseconds
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(stripped, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt.and_utc());
    }
    None
}

/// Compute the duration in milliseconds between two optional timestamp strings.
pub fn duration_between(start: Option<&str>, end: Option<&str>) -> Option<i64> {
    let start_dt = parse_spark_timestamp(start?)?;
    let end_dt = parse_spark_timestamp(end?)?;
    let duration = end_dt - start_dt;
    Some(duration.num_milliseconds())
}

#[cfg(test)]
mod tests {
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
}
