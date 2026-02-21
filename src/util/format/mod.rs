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

/// Format bytes into a human-readable size string, or "-" if zero.
pub fn format_bytes_or_dash(bytes: i64) -> String {
    if bytes > 0 {
        format_bytes(bytes)
    } else {
        "-".to_string()
    }
}

/// Format a record count into a human-readable string.
pub fn format_records(records: i64) -> String {
    if records < 0 {
        return "N/A".to_string();
    }
    let r = records as f64;
    if r < 1_000.0 {
        format!("{}", records)
    } else if r < 1_000_000.0 {
        format!("{:.1}K", r / 1_000.0)
    } else if r < 1_000_000_000.0 {
        format!("{:.1}M", r / 1_000_000.0)
    } else {
        format!("{:.1}B", r / 1_000_000_000.0)
    }
}

/// Compute a percentile from a sorted slice of f64 values.
/// `p` should be between 0.0 and 1.0 (e.g. 0.5 for median, 0.9 for p90).
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let idx = p * (sorted.len() - 1) as f64;
    let lower = idx.floor() as usize;
    let upper = idx.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let frac = idx - lower as f64;
        sorted[lower] * (1.0 - frac) + sorted[upper] * frac
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

/// Replace embedded newlines, carriage returns, and tabs with spaces.
/// Ratatui's `Line`/`Span` types expect no embedded newlines — they corrupt
/// the differential renderer's cursor position tracking.
pub fn sanitize_for_span(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\n' | '\r' | '\t' => ' ',
            _ => c,
        })
        .collect()
}

/// Strip the noisy "Spark Connect - session_id: ..." prefix from stage names.
/// If meaningful code location exists after the prefix, extract it.
pub fn clean_stage_name(name: &str) -> &str {
    const PREFIX: &str = "Spark Connect - session_id: ";
    if let Some(rest) = name.strip_prefix(PREFIX) {
        // The UUID is 36 chars (with hyphens). Skip past it.
        // Format: "uuid" or "uuid - description"
        // Look for closing quote of UUID then a separator
        #[allow(clippy::collapsible_if)]
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
mod tests;
