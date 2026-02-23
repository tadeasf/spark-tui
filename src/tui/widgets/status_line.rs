use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::tui::DataSourceMode;
use crate::tui::theme;

#[allow(clippy::too_many_arguments)]
pub fn render_status_line(
    f: &mut Frame,
    area: Rect,
    cluster_id: &str,
    app_id: &str,
    last_updated: &str,
    data_source: DataSourceMode,
    data_source_detail: Option<&str>,
    error_msg: Option<&str>,
    hint: &str,
) {
    let style = theme::status_bar();

    let spans = if let Some(err) = error_msg {
        vec![
            Span::styled(" spark-tui", style),
            Span::styled(" | ", style),
            Span::styled(err, theme::critical()),
        ]
    } else {
        let time_span = match data_source {
            DataSourceMode::Historical => {
                let label = match data_source_detail {
                    Some(detail) => format!("HISTORICAL ({})", detail),
                    None => "HISTORICAL".to_string(),
                };
                Span::styled(label, theme::warning())
            }
            DataSourceMode::Live => Span::styled(last_updated, style),
        };

        vec![
            Span::styled(" spark-tui", style),
            Span::styled(" | cluster: ", style),
            Span::styled(cluster_id, style),
            Span::styled(" | app: ", style),
            Span::styled(app_id, style),
            Span::styled(" | ", style),
            time_span,
            Span::styled(" | ", style),
            Span::styled(hint, style),
        ]
    };

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(style);
    f.render_widget(paragraph, area);
}
