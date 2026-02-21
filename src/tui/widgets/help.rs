use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::analyze::types::{RankedJob, Suspect};
use crate::tui::theme;

fn centered_rect(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vert = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vert[1])[1]
}

pub fn render_help_overlay(f: &mut Frame, area: Rect) {
    let popup = centered_rect(area, 60, 70);
    f.render_widget(Clear, popup);

    let lines = vec![
        Line::from(Span::styled("Global", theme::tab_active())),
        Line::from("  q          Quit"),
        Line::from("  h          Toggle this help overlay"),
        Line::from("  Esc        Back / dismiss help"),
        Line::from(""),
        Line::from(Span::styled("List View", theme::tab_active())),
        Line::from("  Tab        Switch to next tab"),
        Line::from("  Shift+Tab  Switch to previous tab"),
        Line::from("  j / k      Navigate down / up"),
        Line::from("  g / G      Jump to first / last"),
        Line::from("  Enter      Open detail view"),
        Line::from(""),
        Line::from(Span::styled("Job Detail", theme::tab_active())),
        Line::from("  j / k      Navigate stages"),
        Line::from("  Enter      Open stage detail"),
        Line::from("  s          Open SQL detail"),
        Line::from("  Esc        Back to list"),
        Line::from(""),
        Line::from(Span::styled("SQL / Stage Detail", theme::tab_active())),
        Line::from("  j / k      Scroll down / up"),
        Line::from("  g / G      Jump to top / bottom"),
        Line::from("  Esc        Back to job detail"),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Keybindings (h to close) ");

    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });

    f.render_widget(para, popup);
}

pub fn render_sql_help_overlay(f: &mut Frame, area: Rect, job: &RankedJob, suspects: &[Suspect]) {
    let popup = centered_rect(area, 65, 75);
    f.render_widget(Clear, popup);

    let sql_id = job.sql_id.unwrap_or(0);
    let mut lines: Vec<Line> = Vec::new();

    let sql_suspects: Vec<&Suspect> = suspects
        .iter()
        .filter(|s| s.sql_id == job.sql_id && job.sql_id.is_some())
        .collect();

    if sql_suspects.is_empty() {
        lines.push(Line::from(Span::styled(
            "No issues detected for this SQL query.",
            theme::healthy(),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Recommendations:",
            theme::tab_active(),
        )));
        lines.push(Line::from(""));

        for s in &sql_suspects {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", s.severity),
                    theme::severity_style(s.severity),
                ),
                Span::styled(format!("[Stage {}] ", s.stage_id), theme::muted()),
                Span::raw(&*s.title),
            ]));
            if !s.detail.is_empty() {
                lines.push(Line::from(vec![Span::raw("    "), Span::raw(&*s.detail)]));
            }
            if let Some(rec) = &s.recommendation {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(rec.as_str(), theme::healthy()),
                ]));
            }
            lines.push(Line::from(""));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Quick Keys", theme::tab_active())));
    lines.push(Line::from("  h/Esc  Close this overlay"));
    lines.push(Line::from("  j/k    Scroll SQL detail"));
    lines.push(Line::from("  g/G    Top / bottom"));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" SQL #{sql_id} — Recommendations (h to close) "));

    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });

    f.render_widget(para, popup);
}
