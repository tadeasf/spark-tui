use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, Wrap},
};

use crate::analyze::types::RankedJob;
use crate::fetch::types::SparkStage;
use crate::tui::theme;
use crate::util::format::{
    clean_stage_name, format_bytes_or_dash, format_duration_ms, sanitize_for_span, truncate,
};
use crate::util::time::duration_between;

use super::jobs_list::format_submission_time;

pub fn render_job_detail(
    f: &mut Frame,
    area: Rect,
    job: &RankedJob,
    stages: &[SparkStage],
    _sql_executions: &[crate::fetch::types::SparkSqlExecution],
    stage_state: &mut TableState,
    critical_stages: &std::collections::HashSet<i64>,
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
    let header_para = Paragraph::new(header_lines)
        .block(header_block)
        .wrap(Wrap { trim: true });
    f.render_widget(header_para, layout[0]);

    // -- SQL section --
    if has_sql {
        let sql_id = job.sql_id.unwrap();
        let sql_desc_raw = job.sql_description.as_deref().unwrap_or("(no description)");
        let sql_desc_clean = sanitize_for_span(sql_desc_raw);

        let mut sql_lines = vec![Line::from(vec![
            Span::styled(" SQL: ", theme::tab_active()),
            Span::raw(format!("#{} — {}", sql_id, truncate(&sql_desc_clean, 70))),
        ])];

        // Show first few lines of plan_description
        if let Some(plan) = &job.sql_plan {
            let first_line = plan.lines().next().unwrap_or("(empty plan)");
            sql_lines.push(Line::from(vec![
                Span::styled(" Plan: ", theme::tab_active()),
                Span::raw(truncate(first_line, 80)),
            ]));
            if let Some(second_line) = plan.lines().nth(1) {
                sql_lines.push(Line::from(vec![
                    Span::raw("        "),
                    Span::raw(truncate(second_line.trim_start(), 74)),
                ]));
            }
        }

        let sql_block = Block::default()
            .borders(Borders::ALL)
            .title(format!("SQL #{} [s:expand]", sql_id));
        let sql_para = Paragraph::new(sql_lines)
            .block(sql_block)
            .wrap(Wrap { trim: true });
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

            let input_str = format_bytes_or_dash(s.input_bytes);
            let output_str = format_bytes_or_dash(s.output_bytes);
            let shuf_r_str = format_bytes_or_dash(s.shuffle_read_bytes);
            let shuf_w_str = format_bytes_or_dash(s.shuffle_write_bytes);
            let spill_str = format_bytes_or_dash(s.disk_bytes_spilled);

            let name_str = if critical_stages.contains(&s.stage_id) {
                format!("{} {}", truncate(clean_stage_name(&s.name), 27), "CP")
            } else {
                truncate(clean_stage_name(&s.name), 30)
            };
            let name_style = if critical_stages.contains(&s.stage_id) {
                theme::critical()
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(s.stage_id.to_string()),
                Cell::from(status_str).style(status_style),
                Cell::from(name_str).style(name_style),
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
