# Utilities Module

**Path:** `src/util/`

Helper functions for formatting values and parsing timestamps.

## Files

| File | Purpose |
|------|---------|
| `format.rs` | Human-readable formatting for durations, bytes, strings, and SQL plans |
| `time.rs` | Spark timestamp parsing and duration calculation |

## `format.rs` — Formatting

| Function | Signature | Description |
|----------|-----------|-------------|
| `format_duration_ms` | `(ms: i64) -> String` | Formats milliseconds as human-readable duration (e.g., `1h 23m 45s`, `500ms`, `2.3s`) |
| `format_bytes` | `(bytes: i64) -> String` | Formats byte counts with appropriate unit (e.g., `1.5 GB`, `256 MB`, `1.2 KB`) |
| `truncate` | `(s: &str, max: usize) -> String` | Truncates a string to `max` characters, appending `...` if truncated |
| `clean_stage_name` | `(name: &str) -> String` | Removes Spark Connect prefixes and UUID suffixes from stage names for cleaner display |
| `parse_plan_top_operations` | `(plan: &str, limit: usize) -> Vec<String>` | Extracts the top N operations from a Spark SQL physical plan |

### Examples

```rust
format_duration_ms(3_723_000) // "1h 2m 3s"
format_duration_ms(500)        // "500ms"
format_duration_ms(2_300)      // "2.3s"

format_bytes(1_610_612_736)    // "1.5 GB"
format_bytes(1_048_576)        // "1.0 MB"

truncate("hello world", 5)     // "hello..."

clean_stage_name("spark-connect-UUID:stage_name") // "stage_name"
```

## `time.rs` — Time Utilities

| Function | Signature | Description |
|----------|-----------|-------------|
| `parse_spark_timestamp` | `(s: &str) -> Option<DateTime<Utc>>` | Parses Spark timestamps in multiple formats: RFC3339, naive datetime, and GMT-suffix |
| `duration_between` | `(start: Option<&str>, end: Option<&str>) -> Option<i64>` | Computes duration in milliseconds between two optional timestamp strings |

### Supported Timestamp Formats

- RFC3339: `2024-01-15T10:30:00.000Z`
- Naive: `2024-01-15T10:30:00.000`
- GMT suffix: `2024-01-15T10:30:00.000GMT`

### Examples

```rust
parse_spark_timestamp("2024-01-15T10:30:00.000GMT") // Some(DateTime<Utc>)

duration_between(
    Some("2024-01-15T10:30:00.000GMT"),
    Some("2024-01-15T10:31:00.000GMT"),
) // Some(60_000)

duration_between(Some("..."), None) // None
```
