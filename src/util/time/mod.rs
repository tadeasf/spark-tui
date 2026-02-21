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
mod tests;
