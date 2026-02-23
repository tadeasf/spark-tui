use crate::fetch::types::SparkStage;
use crate::util::format::format_bytes;

use super::super::types::BottleneckPattern;

pub(super) const ONE_MB: i64 = 1_048_576;
pub(super) const FIFTY_MB: i64 = 50 * ONE_MB;
pub(super) const ONE_HUNDRED_MB: i64 = 100 * ONE_MB;
pub(super) const FIVE_HUNDRED_MB: i64 = 500 * ONE_MB;
pub(super) const ONE_GB: i64 = 1024 * ONE_MB;

/// Classify the root-cause bottleneck pattern for a stage.
pub fn classify_bottleneck(s: &SparkStage) -> Option<BottleneckPattern> {
    // DataExplosion: input > 100MB && output > 5 * input
    if s.input_bytes > ONE_HUNDRED_MB && s.output_bytes > 5 * s.input_bytes {
        return Some(BottleneckPattern::DataExplosion);
    }
    // LargeScan: input > 1GB && input > 10 * (output + shuffle_write)
    if s.input_bytes > ONE_GB && s.input_bytes > 10 * (s.output_bytes + s.shuffle_write_bytes) {
        return Some(BottleneckPattern::LargeScan);
    }
    // WideShuffle: shuffle_write > 500MB || (shuffle_read > input && input > 0)
    if s.shuffle_write_bytes > FIVE_HUNDRED_MB
        || (s.shuffle_read_bytes > s.input_bytes && s.input_bytes > 0)
    {
        return Some(BottleneckPattern::WideShuffle);
    }
    None
}

/// Get a PySpark-specific recommendation for a bottleneck.
pub(super) fn bottleneck_recommendation(pattern: BottleneckPattern) -> &'static str {
    match pattern {
        BottleneckPattern::LargeScan => {
            "Filter early: df.filter(F.col('date') >= '2024-01-01') and select only needed columns: df.select('col1', 'col2'). Use partition pruning on date/region columns."
        }
        BottleneckPattern::WideShuffle => {
            "Use broadcast for small tables: from pyspark.sql.functions import broadcast; df.join(broadcast(small_df), ...). Pre-aggregate with groupBy before joins."
        }
        BottleneckPattern::DataExplosion => {
            "Filter before explode, not after: df.filter(...).withColumn('x', explode('arr')). Check for unintentional cross joins — add explicit join conditions."
        }
        BottleneckPattern::RecordExplosion => {
            "Check for explode()/posexplode() on large arrays, or cross joins. Filter input before explode: df.filter(...).select(explode('col'))."
        }
    }
}

/// Build a one-line I/O summary for a stage, including in:out ratio when significant.
pub(super) fn stage_io_summary(s: &SparkStage) -> String {
    let mut parts = Vec::new();
    if s.input_bytes > 0 {
        parts.push(format!("in: {}", format_bytes(s.input_bytes)));
    }
    if s.output_bytes > 0 {
        parts.push(format!("out: {}", format_bytes(s.output_bytes)));
    }
    if s.shuffle_read_bytes > 0 {
        parts.push(format!("shuf_r: {}", format_bytes(s.shuffle_read_bytes)));
    }
    if s.shuffle_write_bytes > 0 {
        parts.push(format!("shuf_w: {}", format_bytes(s.shuffle_write_bytes)));
    }
    if s.input_bytes > 0 && s.output_bytes > 0 {
        let ratio = s.input_bytes as f64 / s.output_bytes as f64;
        if ratio > 2.0 {
            parts.push(format!("ratio: {:.0}:1 in:out", ratio));
        }
    }
    if parts.is_empty() {
        "no I/O".to_string()
    } else {
        parts.join(", ")
    }
}
