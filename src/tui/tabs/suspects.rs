use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};

use crate::analyze::types::{BottleneckPattern, Suspect};
use crate::tui::theme;
use crate::util::format::{clean_stage_name, truncate};

pub fn render_suspects_tab(
    f: &mut Frame,
    area: ratatui::layout::Rect,
    suspects: &[Suspect],
    state: &mut TableState,
) {
    // Split into table (top) and detail panel (bottom, 14 lines)
    let chunks = Layout::vertical([Constraint::Min(5), Constraint::Length(14)]).split(area);

    // -- Table --
    let header_cells = ["Sev", "Category", "Pattern", "Stage", "Job", "SQL", "Title"]
        .iter()
        .map(|h| Cell::from(*h).style(theme::tab_active()));
    let header = Row::new(header_cells).height(1);

    let rows: Vec<Row> = suspects
        .iter()
        .map(|s| {
            let sev_style = theme::severity_style(s.severity);

            let job_str = match s.job_id {
                Some(id) => id.to_string(),
                None => "-".to_string(),
            };

            let sql_str = match s.sql_id {
                Some(id) => format!("#{}", id),
                None => "-".to_string(),
            };

            let (pattern_str, pattern_style) = match s.bottleneck {
                Some(BottleneckPattern::LargeScan) => ("Large Scan".to_string(), theme::warning()),
                Some(BottleneckPattern::WideShuffle) => {
                    ("Wide Shuffle".to_string(), theme::critical())
                }
                Some(BottleneckPattern::DataExplosion) => {
                    ("Data Explosion".to_string(), theme::critical())
                }
                None => ("-".to_string(), theme::muted()),
            };

            Row::new(vec![
                Cell::from(s.severity.to_string()).style(sev_style),
                Cell::from(s.category.to_string()),
                Cell::from(pattern_str).style(pattern_style),
                Cell::from(s.stage_id.to_string()),
                Cell::from(job_str),
                Cell::from(sql_str),
                Cell::from(truncate(&s.title, 50)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(5),
        Constraint::Length(12),
        Constraint::Length(16),
        Constraint::Length(7),
        Constraint::Length(6),
        Constraint::Length(8),
        Constraint::Fill(1),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Suspects (sorted by severity)"),
        )
        .row_highlight_style(theme::selected())
        .highlight_symbol("▶ ");

    f.render_stateful_widget(table, chunks[0], state);

    // -- Detail panel for selected suspect --
    let selected = state.selected().unwrap_or(0);
    let detail_block = Block::default()
        .borders(Borders::ALL)
        .title("Suspect Detail");

    if let Some(s) = suspects.get(selected) {
        let mut lines = Vec::new();

        // Stage line (with cleaned name)
        let stage_label = match &s.stage_name {
            Some(name) => format!("Stage {}: {}", s.stage_id, clean_stage_name(name)),
            None => format!("Stage {}", s.stage_id),
        };
        lines.push(Line::from(vec![
            Span::styled("Stage:   ", theme::tab_active()),
            Span::raw(stage_label),
        ]));

        // SQL line
        if let Some(sql_id) = s.sql_id {
            let desc = s.sql_description.as_deref().unwrap_or("(no description)");
            lines.push(Line::from(vec![
                Span::styled("SQL:     ", theme::tab_active()),
                Span::raw(format!("#{} — {}", sql_id, truncate(desc, 70))),
            ]));
        }

        // Pattern line
        if let Some(pattern) = &s.bottleneck {
            let pattern_style = match pattern {
                BottleneckPattern::LargeScan => theme::warning(),
                BottleneckPattern::WideShuffle | BottleneckPattern::DataExplosion => {
                    theme::critical()
                }
            };
            lines.push(Line::from(vec![
                Span::styled("Pattern: ", theme::tab_active()),
                Span::styled(pattern.to_string(), pattern_style),
            ]));
        }

        // SQL plan operations
        if let Some(hint) = &s.sql_plan_hint {
            lines.push(Line::from(vec![
                Span::styled("Ops:     ", theme::tab_active()),
                Span::styled(truncate(hint, 80), theme::muted()),
            ]));
        }

        // Detail line
        if !s.detail.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Detail:  ", theme::tab_active()),
                Span::raw(truncate(&s.detail, 80)),
            ]));
        }

        // I/O line (styled in cyan)
        if let Some(io) = &s.io_summary {
            lines.push(Line::from(vec![
                Span::styled("I/O:     ", theme::tab_active()),
                Span::styled(io.clone(), theme::running()),
            ]));
        }

        // Recommendation line (styled in green)
        if let Some(rec) = &s.recommendation {
            lines.push(Line::from(vec![
                Span::styled("Fix:     ", theme::tab_active()),
                Span::styled(rec.clone(), theme::healthy()),
            ]));
        }

        let paragraph = Paragraph::new(lines).block(detail_block);
        f.render_widget(paragraph, chunks[1]);
    } else {
        let paragraph = Paragraph::new("No suspect selected.").block(detail_block);
        f.render_widget(paragraph, chunks[1]);
    }
}
