use std::sync::LazyLock;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::{
    easy::HighlightLines, highlighting::ThemeSet, parsing::SyntaxSet, util::LinesWithEndings,
};

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Highlight SQL text using syntect with base16-ocean.dark theme.
pub fn highlight_sql(sql_text: &str) -> Vec<Line<'static>> {
    let ss = &*SYNTAX_SET;
    let ts = &*THEME_SET;

    let syntax = ss
        .find_syntax_by_extension("sql")
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let theme = &ts.themes["base16-ocean.dark"];
    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut lines = Vec::new();

    for line in LinesWithEndings::from(sql_text) {
        let Ok(ranges) = highlighter.highlight_line(line, ss) else {
            lines.push(Line::from(Span::raw(line.to_string())));
            continue;
        };

        let spans: Vec<Span<'static>> = ranges
            .into_iter()
            .map(|(style, text)| {
                let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
                let ratatui_style = Style::default().fg(fg);
                Span::styled(text.to_string(), ratatui_style)
            })
            .collect();

        lines.push(Line::from(spans));
    }

    lines
}

/// Highlight a Spark physical execution plan with keyword-based coloring.
pub fn highlight_spark_plan(plan_text: &str) -> Vec<Line<'static>> {
    plan_text.lines().map(highlight_plan_line).collect()
}

const PLAN_KEYWORDS: &[&str] = &[
    "AdaptiveSparkPlan",
    "AQEShuffleRead",
    "BroadcastExchange",
    "BroadcastHashJoin",
    "BroadcastNestedLoopJoin",
    "CartesianProduct",
    "CoalesceExec",
    "CollectLimit",
    "ColumnarToRow",
    "CustomShuffleReader",
    "Exchange",
    "Expand",
    "Filter",
    "Generate",
    "GlobalLimit",
    "HashAggregate",
    "InMemoryRelation",
    "InMemoryTableScan",
    "LocalLimit",
    "ObjectHashAggregate",
    "PhotonAdapter",
    "PhotonGroupingAgg",
    "PhotonResultStage",
    "PhotonScan",
    "PhotonShuffleExchangeSink",
    "PhotonShuffleExchangeSource",
    "PhotonShuffleMapStage",
    "Project",
    "ReusedExchange",
    "Scan parquet",
    "Scan csv",
    "Scan json",
    "Scan orc",
    "Scan delta",
    "ShuffleExchangeExec",
    "Sort",
    "SortAggregate",
    "SortMergeJoin",
    "SubqueryBroadcast",
    "TakeOrderedAndProject",
    "Union",
    "WholeStageCodegen",
    "Window",
    "WindowExec",
    "WindowGroupLimit",
];

fn highlight_plan_line(line: &str) -> Line<'static> {
    // Section headers like "== Physical Plan =="
    if line.trim_start().starts_with("==") && line.trim_end().ends_with("==") {
        return Line::from(Span::styled(
            line.to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Split into tree-prefix (whitespace + tree markers) and content
    let (prefix, content) = split_tree_prefix(line);

    let mut spans: Vec<Span<'static>> = Vec::new();

    if !prefix.is_empty() {
        spans.push(Span::styled(
            prefix.to_string(),
            Style::default().fg(Color::DarkGray),
        ));
    }

    if content.is_empty() {
        return Line::from(spans);
    }

    tokenize_plan_content(content, &mut spans);
    Line::from(spans)
}

fn split_tree_prefix(line: &str) -> (&str, &str) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'+' | b'-' | b':' | b'|' | b'!' | b'*' => i += 1,
            _ => break,
        }
    }
    (&line[..i], &line[i..])
}

fn tokenize_plan_content(content: &str, spans: &mut Vec<Span<'static>>) {
    let mut pos = 0;
    let bytes = content.as_bytes();

    while pos < bytes.len() {
        // Try matching a keyword at current position
        if let Some(kw_len) = try_match_keyword(content, pos) {
            spans.push(Span::styled(
                content[pos..pos + kw_len].to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ));
            pos += kw_len;
            continue;
        }

        // Try matching a column reference (e.g., col#123, amount#456L)
        if let Some(ref_len) = try_match_col_ref(content, pos) {
            spans.push(Span::styled(
                content[pos..pos + ref_len].to_string(),
                Style::default().fg(Color::Cyan),
            ));
            pos += ref_len;
            continue;
        }

        // Try matching a quoted string
        if let Some(q_len) = try_match_quoted(content, pos) {
            spans.push(Span::styled(
                content[pos..pos + q_len].to_string(),
                Style::default().fg(Color::Yellow),
            ));
            pos += q_len;
            continue;
        }

        // Try matching a standalone number
        if let Some(n_len) = try_match_number(content, pos) {
            spans.push(Span::styled(
                content[pos..pos + n_len].to_string(),
                Style::default().fg(Color::Magenta),
            ));
            pos += n_len;
            continue;
        }

        // Default: consume until next potential token start
        let start = pos;
        pos += 1;
        while pos < bytes.len() && !is_token_start(content, pos) {
            pos += 1;
        }
        spans.push(Span::raw(content[start..pos].to_string()));
    }
}

fn try_match_keyword(content: &str, pos: usize) -> Option<usize> {
    let rest = &content[pos..];
    for kw in PLAN_KEYWORDS {
        if rest.starts_with(kw) {
            let end = pos + kw.len();
            // Ensure the keyword isn't part of a larger word
            if end >= content.len() || !content.as_bytes()[end].is_ascii_alphanumeric() {
                return Some(kw.len());
            }
        }
    }
    None
}

fn try_match_col_ref(content: &str, pos: usize) -> Option<usize> {
    // Pattern: identifier#digits[optional suffix like L]
    let bytes = content.as_bytes();
    if !bytes[pos].is_ascii_alphabetic() && bytes[pos] != b'_' {
        return None;
    }

    let mut i = pos + 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }

    if i >= bytes.len() || bytes[i] != b'#' {
        return None;
    }
    i += 1; // skip #

    if i >= bytes.len() || !bytes[i].is_ascii_digit() {
        return None;
    }
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }

    // Optional type suffix (L, D, etc.)
    if i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }

    Some(i - pos)
}

fn try_match_quoted(content: &str, pos: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let quote = bytes[pos];
    if quote != b'\'' && quote != b'"' {
        return None;
    }

    let mut i = pos + 1;
    while i < bytes.len() {
        if bytes[i] == quote {
            return Some(i - pos + 1);
        }
        if bytes[i] == b'\\' {
            i += 1; // skip escaped char
        }
        i += 1;
    }

    // Unclosed quote — highlight to end of content
    Some(bytes.len() - pos)
}

fn try_match_number(content: &str, pos: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    if !bytes[pos].is_ascii_digit() {
        return None;
    }

    // Make sure it's standalone (not part of a word)
    if pos > 0
        && (bytes[pos - 1].is_ascii_alphanumeric()
            || bytes[pos - 1] == b'_'
            || bytes[pos - 1] == b'#')
    {
        return None;
    }

    let mut i = pos + 1;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
        i += 1;
    }

    // Skip if followed by alphanumeric (part of an identifier)
    if i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
        return None;
    }

    Some(i - pos)
}

fn is_token_start(content: &str, pos: usize) -> bool {
    let bytes = content.as_bytes();
    let b = bytes[pos];

    // Could be start of a keyword
    if b.is_ascii_uppercase() {
        return true;
    }
    // Could be start of a col ref
    if b.is_ascii_lowercase() || b == b'_' {
        // Check if there's a # ahead (rough heuristic)
        return content[pos..].contains('#');
    }
    // Could be a quote
    if b == b'\'' || b == b'"' {
        return true;
    }
    // Could be a standalone number
    if b.is_ascii_digit() {
        if pos > 0
            && (bytes[pos - 1].is_ascii_alphanumeric()
                || bytes[pos - 1] == b'_'
                || bytes[pos - 1] == b'#')
        {
            return false;
        }
        return true;
    }

    false
}
