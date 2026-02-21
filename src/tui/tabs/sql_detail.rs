use ratatui::{
    Frame,
    layout::{Rect, Size},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use tui_scrollview::{ScrollView, ScrollViewState};

use crate::analyze::types::RankedJob;
use crate::tui::{highlight, theme};

pub fn render_sql_detail(
    f: &mut Frame,
    area: Rect,
    job: &RankedJob,
    scroll_state: &mut ScrollViewState,
    suspects: &[crate::analyze::types::Suspect],
) {
    let sql_id = job.sql_id.unwrap_or(0);
    let sql_desc = job.sql_description.as_deref().unwrap_or("(no description)");
    let sql_plan = job.sql_plan.as_deref().unwrap_or("(no plan available)");

    let mut lines: Vec<Line> = Vec::new();

    // Show recommendations for this SQL query at the top
    let sql_suspects: Vec<&crate::analyze::types::Suspect> = suspects
        .iter()
        .filter(|s| s.sql_id == job.sql_id && job.sql_id.is_some())
        .collect();

    if !sql_suspects.is_empty() {
        lines.push(Line::from(Span::styled(
            "Recommendations:",
            theme::tab_active(),
        )));
        for s in &sql_suspects {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", s.severity),
                    theme::severity_style(s.severity),
                ),
                Span::styled(format!("[Stage {}] ", s.stage_id), theme::muted()),
                Span::raw(&s.title),
            ]));
            if let Some(rec) = &s.recommendation {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(rec, theme::healthy()),
                ]));
            }
        }
        lines.push(Line::from(""));
    }

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
