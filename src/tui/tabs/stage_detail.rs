use ratatui::{
    Frame,
    layout::{Rect, Size},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use tui_scrollview::{ScrollView, ScrollViewState};

use std::collections::HashMap;

use crate::fetch::types::{SparkStage, SparkTask};
use crate::tui::theme;
use crate::util::format::{
    clean_stage_name, format_bytes, format_duration_ms, format_records, percentile, truncate,
};
use crate::util::time::duration_between;

fn render_stage_header<'a>(
    stage: &'a SparkStage,
    tasks: Option<&[SparkTask]>,
    total_cluster_memory: i64,
    sql_hint: Option<&str>,
) -> Vec<Line<'a>> {
    let mut lines = Vec::new();

    let dur_str = match duration_between(
        stage.submission_time.as_deref(),
        stage.completion_time.as_deref(),
    ) {
        Some(ms) => format_duration_ms(ms),
        None => "active".to_string(),
    };
    let status_str = format!("{:?}", stage.status).to_uppercase();

    lines.push(Line::from(vec![
        Span::styled(format!(" Stage #{} ", stage.stage_id), theme::tab_active()),
        Span::raw("  "),
        Span::styled(status_str, theme::running()),
        Span::raw("  "),
        Span::raw(format!("Duration: {}  Tasks: {}", dur_str, stage.num_tasks)),
    ]));

    lines.push(Line::from(vec![
        Span::styled(" Name: ", theme::tab_active()),
        Span::raw(clean_stage_name(&stage.name)),
    ]));

    // SQL plan hint
    if let Some(hint) = sql_hint {
        lines.push(Line::from(vec![
            Span::styled(" SQL:  ", theme::tab_active()),
            Span::styled(hint.to_string(), theme::muted()),
        ]));
    }

    // I/O metrics
    let mut io_parts = Vec::new();
    if stage.input_bytes > 0 {
        io_parts.push(format!(
            "in: {} ({})",
            format_bytes(stage.input_bytes),
            format_records(stage.input_records)
        ));
    }
    if stage.output_bytes > 0 {
        io_parts.push(format!(
            "out: {} ({})",
            format_bytes(stage.output_bytes),
            format_records(stage.output_records)
        ));
    }
    if stage.shuffle_read_bytes > 0 {
        io_parts.push(format!(
            "shuf_r: {} ({})",
            format_bytes(stage.shuffle_read_bytes),
            format_records(stage.shuffle_read_records)
        ));
    }
    if stage.shuffle_write_bytes > 0 {
        io_parts.push(format!(
            "shuf_w: {} ({})",
            format_bytes(stage.shuffle_write_bytes),
            format_records(stage.shuffle_write_records)
        ));
    }
    if stage.disk_bytes_spilled > 0 {
        io_parts.push(format!(
            "spill: {} (RAM: {})",
            format_bytes(stage.disk_bytes_spilled),
            format_bytes(stage.memory_bytes_spilled)
        ));
    }
    if !io_parts.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(" I/O:  ", theme::tab_active()),
            Span::raw(io_parts.join("  ")),
        ]));
    }

    // CPU efficiency
    if stage.executor_run_time > 0 {
        let cpu_ms = stage.executor_cpu_time / 1_000_000;
        let ratio = cpu_ms as f64 / stage.executor_run_time as f64;
        let cpu_style = theme::cpu_utilization_style(ratio);
        lines.push(Line::from(vec![
            Span::styled(" CPU:  ", theme::tab_active()),
            Span::styled(format!("{:.0}%", ratio * 100.0), cpu_style),
            Span::raw(format!(
                " utilization (cpu: {} / runtime: {})",
                format_duration_ms(cpu_ms),
                format_duration_ms(stage.executor_run_time),
            )),
        ]));
    }

    // Peak execution memory — stage-level with task-data fallback
    let peak_mem = if stage.peak_execution_memory > 0 {
        Some((stage.peak_execution_memory, "stage"))
    } else {
        tasks
            .and_then(|t| t.iter().map(|t| t.peak_execution_memory).max())
            .filter(|&max| max > 0)
            .map(|max| (max, "task max"))
    };

    if let Some((mem_bytes, source)) = peak_mem {
        let mem_style = theme::memory_utilization_style(mem_bytes, total_cluster_memory);
        lines.push(Line::from(vec![
            Span::styled(" RAM:  ", theme::tab_active()),
            Span::raw("Peak execution RAM: "),
            Span::styled(format_bytes(mem_bytes), mem_style),
            Span::styled(format!(" ({})", source), theme::muted()),
        ]));
    }

    // Task failure info
    if stage.num_failed_tasks > 0 || stage.num_killed_tasks > 0 {
        lines.push(Line::from(vec![
            Span::styled(" Fail: ", theme::critical()),
            Span::styled(
                format!(
                    "{} failed, {} killed out of {} tasks",
                    stage.num_failed_tasks, stage.num_killed_tasks, stage.num_tasks
                ),
                theme::failed(),
            ),
        ]));
    }

    lines
}

fn render_duration_histogram<'a>(durations: &[f64]) -> Vec<Line<'a>> {
    let mut lines = Vec::new();
    if durations.len() < 2 {
        return lines;
    }

    let min = durations[0];
    let max = durations[durations.len() - 1];
    let med = percentile(durations, 0.5);
    let p90 = percentile(durations, 0.9);
    let p99 = percentile(durations, 0.99);

    lines.push(Line::from(Span::styled(
        " Task Duration Distribution",
        theme::tab_active(),
    )));
    lines.push(Line::from(format!(
        "   min: {}  median: {}  p90: {}  p99: {}  max: {}",
        format_duration_ms(min as i64),
        format_duration_ms(med as i64),
        format_duration_ms(p90 as i64),
        format_duration_ms(p99 as i64),
        format_duration_ms(max as i64),
    )));

    let range = max - min;
    if range > 0.0 {
        let num_buckets = 10;
        let bucket_width = range / num_buckets as f64;
        let mut buckets = vec![0usize; num_buckets];
        for &d in durations {
            let idx = ((d - min) / bucket_width).floor() as usize;
            let idx = idx.min(num_buckets - 1);
            buckets[idx] += 1;
        }
        let max_count = *buckets.iter().max().unwrap_or(&1);
        let bar_max_width = 40;

        lines.push(Line::from(""));
        for (i, &count) in buckets.iter().enumerate() {
            let lo = min + i as f64 * bucket_width;
            let hi = lo + bucket_width;
            let bar_len = if max_count > 0 {
                (count as f64 / max_count as f64 * bar_max_width as f64) as usize
            } else {
                0
            };
            let bar: String = "█".repeat(bar_len) + &"░".repeat(bar_max_width - bar_len);
            lines.push(Line::from(format!(
                "   {:>8}-{:<8} |{bar}| {}",
                format_duration_ms(lo as i64),
                format_duration_ms(hi as i64),
                count,
            )));
        }
    }

    lines
}

fn render_executor_breakdown<'a>(tasks: &[SparkTask]) -> Vec<Line<'a>> {
    let mut lines = Vec::new();

    let mut exec_data: HashMap<&str, (i64, i64, i64)> = HashMap::new();
    for t in tasks {
        let entry = exec_data.entry(t.executor_id.as_str()).or_insert((0, 0, 0));
        entry.0 += 1;
        entry.1 += t.duration.unwrap_or(0);
        entry.2 += t.input_bytes + t.shuffle_read_bytes;
    }
    let total_bytes: i64 = exec_data.values().map(|e| e.2).sum();

    let mut exec_rows: Vec<(&str, i64, i64, i64, f64)> = exec_data
        .into_iter()
        .map(|(id, (count, dur, bytes))| {
            let pct = if total_bytes > 0 {
                bytes as f64 / total_bytes as f64 * 100.0
            } else {
                0.0
            };
            (id, count, dur, bytes, pct)
        })
        .collect();
    exec_rows.sort_by(|a, b| b.4.total_cmp(&a.4));

    lines.push(Line::from(Span::styled(
        " Per-Executor Breakdown",
        theme::tab_active(),
    )));
    lines.push(Line::from(format!(
        "   {:<12} {:>6} {:>12} {:>12} {:>8}",
        "Executor", "Tasks", "Avg Dur", "Bytes", "% Data"
    )));

    for (id, count, dur, bytes, pct) in &exec_rows {
        let avg_dur = if *count > 0 { dur / count } else { 0 };
        lines.push(Line::from(format!(
            "   {:<12} {:>6} {:>12} {:>12} {:>7.1}%",
            truncate(id, 12),
            count,
            format_duration_ms(avg_dur),
            format_bytes(*bytes),
            pct,
        )));
    }

    lines
}

fn render_peak_memory_section<'a>(tasks: &[SparkTask], total_cluster_memory: i64) -> Vec<Line<'a>> {
    let mut lines = Vec::new();
    let max_mem = tasks
        .iter()
        .map(|t| t.peak_execution_memory)
        .max()
        .unwrap_or(0);
    if max_mem > 0 {
        let total_mem: i64 = tasks.iter().map(|t| t.peak_execution_memory).sum();
        let avg_mem = total_mem / tasks.len() as i64;
        let mem_style = theme::memory_utilization_style(max_mem, total_cluster_memory);
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Peak Execution Memory (per task)",
            theme::tab_active(),
        )));
        lines.push(Line::from(vec![
            Span::raw("   max: "),
            Span::styled(format_bytes(max_mem), mem_style),
            Span::raw(format!(
                "  avg: {}  total: {}",
                format_bytes(avg_mem),
                format_bytes(total_mem),
            )),
        ]));
    }
    lines
}

fn render_skew_metrics<'a>(durations: &[f64], tasks: &[SparkTask]) -> Vec<Line<'a>> {
    let mut lines = Vec::new();
    if durations.len() < 2 {
        return lines;
    }

    let n = durations.len() as f64;
    let mean = durations.iter().sum::<f64>() / n;
    let variance = durations.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / n;
    let stddev = variance.sqrt();
    let cv = if mean > 0.0 { stddev / mean } else { 0.0 };
    let median = percentile(durations, 0.5);
    let max_val = durations.last().copied().unwrap_or(0.0);
    let max_median_ratio = if median > 0.0 { max_val / median } else { 0.0 };

    let slowest_task = tasks
        .iter()
        .max_by_key(|t| t.duration.unwrap_or(0))
        .unwrap();

    lines.push(Line::from(Span::styled(
        " Skew Metrics",
        theme::tab_active(),
    )));
    lines.push(Line::from(format!(
        "   CV: {:.2}  Max/Median: {:.1}x  Slowest: task #{} on executor {}",
        cv, max_median_ratio, slowest_task.task_id, slowest_task.executor_id,
    )));

    lines
}

#[allow(clippy::too_many_arguments)]
pub fn render_stage_detail(
    f: &mut Frame,
    area: Rect,
    stage: &SparkStage,
    tasks: Option<&[SparkTask]>,
    loading: bool,
    scroll_state: &mut ScrollViewState,
    total_cluster_memory: i64,
    sql_hint: Option<&str>,
) {
    let mut lines = render_stage_header(stage, tasks, total_cluster_memory, sql_hint);
    lines.push(Line::from(""));

    match tasks {
        Some(tasks) if tasks.len() >= 2 => {
            let mut durations: Vec<f64> = tasks
                .iter()
                .filter_map(|t| t.duration.map(|d| d as f64))
                .collect();
            durations.sort_by(|a, b| a.total_cmp(b));

            lines.extend(render_duration_histogram(&durations));
            lines.push(Line::from(""));
            lines.extend(render_executor_breakdown(tasks));
            lines.extend(render_peak_memory_section(tasks, total_cluster_memory));
            lines.push(Line::from(""));
            lines.extend(render_skew_metrics(&durations, tasks));
        }
        Some(_) => {
            lines.push(Line::from(Span::styled(
                " Too few tasks for distribution analysis.",
                theme::muted(),
            )));
        }
        None => {
            if loading {
                lines.push(Line::from(Span::styled(
                    " Loading task data...",
                    theme::muted(),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    " Task data not fetched for this stage.",
                    theme::muted(),
                )));
            }
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Stage #{} Detail", stage.stage_id));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Calculate wrapped content height
    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    let content_height = para.line_count(inner.width) as u16;

    // Render into off-screen ScrollView buffer
    let content_size = Size::new(inner.width, content_height);
    let mut scroll_view = ScrollView::new(content_size);
    scroll_view.render_widget(para, Rect::new(0, 0, inner.width, content_height));

    // Render ScrollView viewport to frame
    f.render_stateful_widget(scroll_view, inner, scroll_state);
}
