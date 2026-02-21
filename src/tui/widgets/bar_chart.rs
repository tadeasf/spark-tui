use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Sparkline},
};

/// Render a simple sparkline bar chart of job durations.
pub fn render_duration_chart(f: &mut Frame, area: Rect, durations_ms: &[u64], title: &str) {
    let sparkline = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::raw(title)),
        )
        .data(durations_ms)
        .style(Style::default().fg(Color::Cyan));

    f.render_widget(sparkline, area);
}
