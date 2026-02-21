use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    widgets::{Block, Borders, Cell, Row, Table, TableState},
};

use crate::analyze::types::RankedJob;
use crate::tui::theme;
use crate::util::format::{format_duration_ms, truncate};
use crate::util::time::parse_spark_timestamp;

pub(super) fn format_submission_time(ts: Option<&str>) -> String {
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
