use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use crate::analyze::types::RankedJob;
use crate::fetch::types::{SparkSqlExecution, SparkStage};
use crate::tui::theme;
use crate::util::format::{clean_stage_name, format_bytes, format_duration_ms, truncate};
use crate::util::time::{duration_between, parse_spark_timestamp};

fn format_submission_time(ts: Option<&str>) -> String {
    ts.and_then(parse_spark_timestamp)
        .map(|dt| dt.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "-".to_string())
}

pub fn render_jobs_tab(
    f: &mut Frame,
    area: Rect,
    jobs: &[RankedJob],
    state: &mut TableState,
) {
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
        Constraint::Fill(1),   // Name
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
        Constraint::Length(3),              // Header
        Constraint::Length(sql_height),     // SQL section (0 if absent)
        Constraint::Fill(1),               // Stages table
    ])
    .split(area);

    // -- Header --
    let header_lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" Job #{} ", job.job_id),
                theme::tab_active(),
            ),
            Span::raw("  "),
            Span::styled(&job.status, status_style),
            Span::raw("  "),
            Span::raw(format!("{}  Started {}  Tasks: {}", duration_str, started_str, job.num_tasks)),
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
        let sql_desc = job
            .sql_description
            .as_deref()
            .unwrap_or("(no description)");

        let mut sql_lines = vec![Line::from(vec![
            Span::styled(" SQL: ", theme::tab_active()),
            Span::raw(format!("#{} — {}", sql_id, truncate(sql_desc, 70))),
        ])];

        // Show first few lines of plan_description
        if let Some(plan) = &job.sql_plan {
            let plan_preview: String = plan
                .lines()
                .take(2)
                .collect::<Vec<_>>()
                .join(" | ");
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
        "ID", "Status", "Name", "Duration", "Tasks", "Input", "Output", "Shuf Read", "Shuf Write", "Spill",
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

            let input_str = if s.input_bytes > 0 { format_bytes(s.input_bytes) } else { "-".to_string() };
            let output_str = if s.output_bytes > 0 { format_bytes(s.output_bytes) } else { "-".to_string() };
            let shuf_r_str = if s.shuffle_read_bytes > 0 { format_bytes(s.shuffle_read_bytes) } else { "-".to_string() };
            let shuf_w_str = if s.shuffle_write_bytes > 0 { format_bytes(s.shuffle_write_bytes) } else { "-".to_string() };
            let spill_str = if s.disk_bytes_spilled > 0 { format_bytes(s.disk_bytes_spilled) } else { "-".to_string() };

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
        Constraint::Fill(1),   // Name
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
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Stages"),
        )
        .row_highlight_style(theme::selected())
        .highlight_symbol("▶ ");

    f.render_stateful_widget(stage_table, layout[2], stage_state);
}

pub fn render_sql_detail(f: &mut Frame, area: Rect, job: &RankedJob, scroll: u16) {
    let sql_id = job.sql_id.unwrap_or(0);
    let sql_desc = job
        .sql_description
        .as_deref()
        .unwrap_or("(no description)");
    let sql_plan = job.sql_plan.as_deref().unwrap_or("(no plan available)");

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("Description: ", theme::tab_active()),
    ]));
    for line in sql_desc.lines() {
        lines.push(Line::from(Span::raw(line.to_string())));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Execution Plan: ", theme::tab_active()),
    ]));
    for line in sql_plan.lines() {
        lines.push(Line::from(Span::raw(line.to_string())));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("SQL #{}", sql_id));

    let para = Paragraph::new(lines)
        .block(block)
        .scroll((scroll, 0));

    f.render_widget(para, area);
}
