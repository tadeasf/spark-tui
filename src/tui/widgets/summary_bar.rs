use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::analyze::types::HealthSummary;
use crate::util::format::format_bytes;

pub fn render_summary_bar(f: &mut Frame, area: Rect, summary: &HealthSummary) {
    let fg_color = if summary.critical_count > 0 {
        Color::Red
    } else if summary.warning_count > 0 {
        Color::Yellow
    } else {
        Color::Green
    };

    let base_style = Style::default().fg(fg_color);

    // Line 1: Jobs | I/O | Health counts
    let job_info = if summary.running_jobs > 0 || summary.failed_jobs > 0 {
        format!(
            "Jobs: {} ({} running, {} failed)",
            summary.total_jobs, summary.running_jobs, summary.failed_jobs
        )
    } else {
        format!("Jobs: {}", summary.total_jobs)
    };

    let io_info = format!(
        "I/O: in {} out {} shuffle {}",
        format_bytes(summary.total_input_bytes),
        format_bytes(summary.total_output_bytes),
        format_bytes(summary.total_shuffle_bytes),
    );

    let health_info = if summary.critical_count > 0 || summary.warning_count > 0 {
        format!(
            "Health: {} CRIT {} WARN",
            summary.critical_count, summary.warning_count
        )
    } else {
        "Health: OK".to_string()
    };

    let line1 = Line::from(vec![
        Span::styled(format!(" {} | {} | {}", job_info, io_info, health_info), base_style),
    ]);

    // Line 2: Top critical issues or "No issues"
    let line2 = if !summary.top_issues.is_empty() {
        Line::from(vec![Span::styled(
            format!(" Top: {}", summary.top_issues.join(", ")),
            base_style,
        )])
    } else {
        Line::from(vec![Span::styled(
            " No critical issues detected".to_string(),
            base_style,
        )])
    };

    let para = Paragraph::new(vec![line1, line2]);
    f.render_widget(para, area);
}
