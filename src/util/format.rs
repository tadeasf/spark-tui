/// Format milliseconds into a human-readable duration string.
pub fn format_duration_ms(ms: i64) -> String {
    if ms < 0 {
        return "N/A".to_string();
    }
    if ms < 1000 {
        return format!("{}ms", ms);
    }
    let secs = ms / 1000;
    if secs < 60 {
        let remainder_ms = ms % 1000;
        if remainder_ms > 0 {
            return format!("{}.{}s", secs, remainder_ms / 100);
        }
        return format!("{}s", secs);
    }
    let mins = secs / 60;
    let rem_secs = secs % 60;
    if mins < 60 {
        return format!("{}m {}s", mins, rem_secs);
    }
    let hours = mins / 60;
    let rem_mins = mins % 60;
    format!("{}h {}m", hours, rem_mins)
}

/// Format bytes into a human-readable size string.
pub fn format_bytes(bytes: i64) -> String {
    if bytes < 0 {
        return "N/A".to_string();
    }
    let bytes = bytes as f64;
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;

    if bytes < KB {
        format!("{} B", bytes as i64)
    } else if bytes < MB {
        format!("{:.1} KB", bytes / KB)
    } else if bytes < GB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes < TB {
        format!("{:.1} GB", bytes / GB)
    } else {
        format!("{:.1} TB", bytes / TB)
    }
}

/// Truncate a string to the given max length, appending "..." if truncated.
pub fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len <= 3 {
        s.chars().take(max_len).collect()
    } else {
        let mut result: String = s.chars().take(max_len - 3).collect();
        result.push_str("...");
        result
    }
}

/// Strip the noisy "Spark Connect - session_id: ..." prefix from stage names.
/// If meaningful code location exists after the prefix, extract it.
pub fn clean_stage_name(name: &str) -> &str {
    const PREFIX: &str = "Spark Connect - session_id: ";
    if let Some(rest) = name.strip_prefix(PREFIX) {
        // The UUID is 36 chars (with hyphens). Skip past it.
        // Format: "uuid" or "uuid - description"
        // Look for closing quote of UUID then a separator
        if let Some(after_uuid) = rest.find("\" - ").map(|i| &rest[i + 4..]) {
            if !after_uuid.is_empty() {
                return after_uuid.trim();
            }
        }
    }
    name
}

/// Parse a Spark SQL physical plan tree and extract the top N operation names.
/// Returns deduplicated operation keywords like ["Scan parquet db.table", "Exchange", "Sort"].
pub fn parse_plan_top_operations(plan: &str, n: usize) -> Vec<String> {
    let mut ops = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for line in plan.lines() {
        // Strip tree markers and whitespace
        let trimmed = line
            .trim()
            .trim_start_matches('+')
            .trim_start_matches('-')
            .trim_start_matches(':')
            .trim();

        if trimmed.is_empty() || trimmed.starts_with("==") {
            continue;
        }

        // Extract operation keyword: everything before the first '(' or '['
        let op_name = if let Some(paren_pos) = trimmed.find('(') {
            trimmed[..paren_pos].trim()
        } else if let Some(bracket_pos) = trimmed.find('[') {
            trimmed[..bracket_pos].trim()
        } else {
            trimmed
        };

        if !op_name.is_empty() && seen.insert(op_name.to_string()) {
            ops.push(op_name.to_string());
            if ops.len() >= n {
                break;
            }
        }
    }

    ops
}

#[cfg(test)]
mod tests {
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
}
