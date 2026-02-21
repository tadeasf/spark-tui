use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};

use std::collections::HashMap;

use crate::analyze::types::RankedJob;
use crate::fetch::types::{SparkSqlExecution, SparkStage, SparkTask};
use crate::tui::{highlight, theme};
use crate::util::format::{
    clean_stage_name, format_bytes, format_duration_ms, format_records, percentile, truncate,
};
use crate::util::time::{duration_between, parse_spark_timestamp};

fn format_submission_time(ts: Option<&str>) -> String {
    ts.and_then(parse_spark_timestamp)
        .map(|dt| dt.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "-".to_string())
}

pub fn render_jobs_tab(f: &mut Frame, area: Rect, jobs: &[RankedJob], state: &mut TableState) {
    let header_cells = [
        "ID", "Status", "Started", "Duration", "Tasks", "Failed", "SQL", "Name",
    ]
    .iter()
    .map(|h| Cell::from(*h).style(theme::tab_active()));
    let header = Row::new(header_cells).height(1);

    let rows: Vec<Row> = jobs
        .iter()
        .map(|job| {
            let status_str = &job.status;
            let status_style = match status_str.as_str() {
                "RUNNING" => theme::running(),
                "SUCCEEDED" => theme::healthy(),
                "FAILED" => theme::failed(),
                _ => theme::muted(),
            };

            let duration_str = match job.duration_ms {
                Some(ms) => format_duration_ms(ms),
                None => "running".to_string(),
            };

            let started_str = format_submission_time(job.submission_time.as_deref());

            let sql_str = match job.sql_id {
                Some(id) => format!("#{}", id),
                None => "-".to_string(),
            };

            Row::new(vec![
                Cell::from(job.job_id.to_string()),
                Cell::from(status_str.clone()).style(status_style),
                Cell::from(started_str),
                Cell::from(duration_str),
                Cell::from(job.num_tasks.to_string()),
                Cell::from(job.num_failed_tasks.to_string()),
                Cell::from(sql_str),
                Cell::from(truncate(&job.name, 50)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(6),  // ID
        Constraint::Length(10), // Status
        Constraint::Length(10), // Started
        Constraint::Length(12), // Duration
        Constraint::Length(8),  // Tasks
        Constraint::Length(7),  // Failed
        Constraint::Length(6),  // SQL
        Constraint::Fill(1),    // Name
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Jobs (sorted by duration)"),
        )
        .row_highlight_style(theme::selected())
        .highlight_symbol("▶ ");

    f.render_stateful_widget(table, area, state);
}

pub fn render_job_detail(
    f: &mut Frame,
    area: Rect,
    job: &RankedJob,
    stages: &[SparkStage],
    _sql_executions: &[SparkSqlExecution],
    stage_state: &mut TableState,
) {
    let duration_str = match job.duration_ms {
        Some(ms) => format_duration_ms(ms),
        None => "running".to_string(),
    };
    let started_str = format_submission_time(job.submission_time.as_deref());

    let status_style = match job.status.as_str() {
        "RUNNING" => theme::running(),
        "SUCCEEDED" => theme::healthy(),
        "FAILED" => theme::failed(),
        _ => theme::muted(),
    };

    // Determine if we have SQL info to show
    let has_sql = job.sql_id.is_some();
    let sql_height = if has_sql { 5 } else { 0 };

    let layout = Layout::vertical([
        Constraint::Length(3),          // Header
        Constraint::Length(sql_height), // SQL section (0 if absent)
        Constraint::Fill(1),            // Stages table
    ])
    .split(area);

    // -- Header --
    let header_lines = vec![
        Line::from(vec![
            Span::styled(format!(" Job #{} ", job.job_id), theme::tab_active()),
            Span::raw("  "),
            Span::styled(&job.status, status_style),
            Span::raw("  "),
            Span::raw(format!(
                "{}  Started {}  Tasks: {}",
                duration_str, started_str, job.num_tasks
            )),
        ]),
        Line::from(vec![
            Span::styled(" Name: ", theme::tab_active()),
            Span::raw(truncate(&job.name, 80)),
        ]),
    ];
    let header_block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Job #{}", job.job_id));
    let header_para = Paragraph::new(header_lines).block(header_block);
    f.render_widget(header_para, layout[0]);

    // -- SQL section --
    if has_sql {
        let sql_id = job.sql_id.unwrap();
        let sql_desc = job.sql_description.as_deref().unwrap_or("(no description)");

        let mut sql_lines = vec![Line::from(vec![
            Span::styled(" SQL: ", theme::tab_active()),
            Span::raw(format!("#{} — {}", sql_id, truncate(sql_desc, 70))),
        ])];

        // Show first few lines of plan_description
        if let Some(plan) = &job.sql_plan {
            let plan_preview: String = plan.lines().take(2).collect::<Vec<_>>().join(" | ");
            sql_lines.push(Line::from(vec![
                Span::styled(" Plan: ", theme::tab_active()),
                Span::raw(truncate(&plan_preview, 80)),
            ]));
        }

        let sql_block = Block::default()
            .borders(Borders::ALL)
            .title(format!("SQL #{} [s:expand]", sql_id));
        let sql_para = Paragraph::new(sql_lines).block(sql_block);
        f.render_widget(sql_para, layout[1]);
    }

    // -- Stages table --
    let job_stages: Vec<&SparkStage> = stages
        .iter()
        .filter(|s| job.stage_ids.contains(&s.stage_id))
        .collect();

    let stage_header_cells = [
        "ID",
        "Status",
        "Name",
        "Duration",
        "Tasks",
        "Input",
        "Output",
        "Shuf Read",
        "Shuf Write",
        "Spill",
    ]
    .iter()
    .map(|h| Cell::from(*h).style(theme::tab_active()));
    let stage_header = Row::new(stage_header_cells).height(1);

    let stage_rows: Vec<Row> = job_stages
        .iter()
        .map(|s| {
            let status_str = format!("{:?}", s.status).to_uppercase();
            let status_style = match s.status {
                crate::fetch::types::StageStatus::Active => theme::running(),
                crate::fetch::types::StageStatus::Complete => theme::healthy(),
                crate::fetch::types::StageStatus::Failed => theme::failed(),
                _ => theme::muted(),
            };

            let dur_str = match duration_between(
                s.submission_time.as_deref(),
                s.completion_time.as_deref(),
            ) {
                Some(ms) => format_duration_ms(ms),
                None => "active".to_string(),
            };

            let input_str = if s.input_bytes > 0 {
                format_bytes(s.input_bytes)
            } else {
                "-".to_string()
            };
            let output_str = if s.output_bytes > 0 {
                format_bytes(s.output_bytes)
            } else {
                "-".to_string()
            };
            let shuf_r_str = if s.shuffle_read_bytes > 0 {
                format_bytes(s.shuffle_read_bytes)
            } else {
                "-".to_string()
            };
            let shuf_w_str = if s.shuffle_write_bytes > 0 {
                format_bytes(s.shuffle_write_bytes)
            } else {
                "-".to_string()
            };
            let spill_str = if s.disk_bytes_spilled > 0 {
                format_bytes(s.disk_bytes_spilled)
            } else {
                "-".to_string()
            };

            Row::new(vec![
                Cell::from(s.stage_id.to_string()),
                Cell::from(status_str).style(status_style),
                Cell::from(truncate(clean_stage_name(&s.name), 30)),
                Cell::from(dur_str),
                Cell::from(s.num_tasks.to_string()),
                Cell::from(input_str).style(theme::metric_bytes_style(s.input_bytes)),
                Cell::from(output_str).style(theme::metric_bytes_style(s.output_bytes)),
                Cell::from(shuf_r_str).style(theme::shuffle_bytes_style(s.shuffle_read_bytes)),
                Cell::from(shuf_w_str).style(theme::shuffle_bytes_style(s.shuffle_write_bytes)),
                Cell::from(spill_str).style(theme::spill_bytes_style(s.disk_bytes_spilled)),
            ])
        })
        .collect();

    let stage_widths = [
        Constraint::Length(5),  // ID
        Constraint::Length(10), // Status
        Constraint::Fill(1),    // Name
        Constraint::Length(10), // Duration
        Constraint::Length(7),  // Tasks
        Constraint::Length(10), // Input
        Constraint::Length(10), // Output
        Constraint::Length(10), // Shuf Read
        Constraint::Length(10), // Shuf Write
        Constraint::Length(10), // Spill
    ];

    let stage_table = Table::new(stage_rows, stage_widths)
        .header(stage_header)
        .block(Block::default().borders(Borders::ALL).title("Stages"))
        .row_highlight_style(theme::selected())
        .highlight_symbol("▶ ");

    f.render_stateful_widget(stage_table, layout[2], stage_state);
}

pub fn render_sql_detail(f: &mut Frame, area: Rect, job: &RankedJob, scroll: u16) {
    let sql_id = job.sql_id.unwrap_or(0);
    let sql_desc = job.sql_description.as_deref().unwrap_or("(no description)");
    let sql_plan = job.sql_plan.as_deref().unwrap_or("(no plan available)");

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![Span::styled(
        "Description: ",
        theme::tab_active(),
    )]));
    lines.extend(highlight::highlight_sql(sql_desc));

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "Execution Plan: ",
        theme::tab_active(),
    )]));
    lines.extend(highlight::highlight_spark_plan(sql_plan));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("SQL #{}", sql_id));

    // Clamp scroll to avoid overflow inside ratatui's Paragraph rendering
    let content_height = lines.len() as u16;
    let visible_height = area.height.saturating_sub(2); // subtract border
    let max_scroll = content_height.saturating_sub(visible_height);
    let clamped_scroll = scroll.min(max_scroll);

    let para = Paragraph::new(lines)
        .block(block)
        .scroll((clamped_scroll, 0));

    f.render_widget(para, area);
}

pub fn render_stage_detail(
    f: &mut Frame,
    area: Rect,
    stage: &SparkStage,
    tasks: Option<&[SparkTask]>,
    loading: bool,
    scroll: u16,
    total_cluster_memory: i64,
) {
    let mut lines: Vec<Line> = Vec::new();

    // -- Stage summary header --
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
        Span::styled(&status_str, theme::running()),
        Span::raw("  "),
        Span::raw(format!("Duration: {}  Tasks: {}", dur_str, stage.num_tasks)),
    ]));

    lines.push(Line::from(vec![
        Span::styled(" Name: ", theme::tab_active()),
        Span::raw(clean_stage_name(&stage.name)),
    ]));

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
    } else if let Some(t) = tasks {
        let max = t.iter().map(|t| t.peak_execution_memory).max().unwrap_or(0);
        if max > 0 {
            Some((max, "task max"))
        } else {
            None
        }
    } else {
        None
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

    lines.push(Line::from(""));

    // -- Task analysis section --
    match tasks {
        Some(tasks) if tasks.len() >= 2 => {
            // Duration histogram
            let mut durations: Vec<f64> = tasks
                .iter()
                .filter_map(|t| t.duration.map(|d| d as f64))
                .collect();
            durations.sort_by(|a, b| a.partial_cmp(b).unwrap());

            if durations.len() >= 2 {
                let min = durations[0];
                let max = durations[durations.len() - 1];
                let med = percentile(&durations, 0.5);
                let p90 = percentile(&durations, 0.9);
                let p99 = percentile(&durations, 0.99);

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

                // Text-based histogram with 10 buckets
                let range = max - min;
                if range > 0.0 {
                    let num_buckets = 10;
                    let bucket_width = range / num_buckets as f64;
                    let mut buckets = vec![0usize; num_buckets];
                    for &d in &durations {
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
                        let bar: String =
                            "█".repeat(bar_len) + &"░".repeat(bar_max_width - bar_len);
                        lines.push(Line::from(format!(
                            "   {:>8}-{:<8} |{bar}| {}",
                            format_duration_ms(lo as i64),
                            format_duration_ms(hi as i64),
                            count,
                        )));
                    }
                }
            }

            lines.push(Line::from(""));

            // -- Per-executor breakdown --
            let mut exec_data: HashMap<&str, (i64, i64, i64)> = HashMap::new();
            for t in tasks {
                let entry = exec_data.entry(t.executor_id.as_str()).or_insert((0, 0, 0));
                entry.0 += 1; // task count
                entry.1 += t.duration.unwrap_or(0); // total duration
                entry.2 += t.input_bytes + t.shuffle_read_bytes; // total bytes
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
            exec_rows.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap());

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

            // -- Peak execution memory (task-level) --
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

            lines.push(Line::from(""));

            // -- Skew metrics --
            if durations.len() >= 2 {
                let n = durations.len() as f64;
                let mean = durations.iter().sum::<f64>() / n;
                let variance = durations.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / n;
                let stddev = variance.sqrt();
                let cv = if mean > 0.0 { stddev / mean } else { 0.0 };
                let median = percentile(&durations, 0.5);
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
            }
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

    let content_height = lines.len() as u16;
    let visible_height = area.height.saturating_sub(2);
    let max_scroll = content_height.saturating_sub(visible_height);
    let clamped_scroll = scroll.min(max_scroll);

    let para = Paragraph::new(lines)
        .block(block)
        .scroll((clamped_scroll, 0));

    f.render_widget(para, area);
}
