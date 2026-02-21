use ratatui::style::{Color, Modifier, Style};

use crate::analyze::types::Severity;
use crate::fetch::types::JobStatus;

pub fn critical() -> Style {
    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
}

pub fn warning() -> Style {
    Style::default().fg(Color::Yellow)
}

pub fn healthy() -> Style {
    Style::default().fg(Color::Green)
}

pub fn running() -> Style {
    Style::default().fg(Color::Cyan)
}

pub fn failed() -> Style {
    Style::default().fg(Color::Red)
}

pub fn muted() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn selected() -> Style {
    Style::default()
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD)
}

pub fn tab_active() -> Style {
    Style::default()
        .fg(Color::White)
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD)
}

pub fn tab_inactive() -> Style {
    Style::default().fg(Color::Gray)
}

pub fn status_bar() -> Style {
    Style::default().fg(Color::White).bg(Color::DarkGray)
}

pub fn severity_style(severity: Severity) -> Style {
    match severity {
        Severity::Critical => critical(),
        Severity::Warning => warning(),
    }
}

pub fn job_status_style(status: JobStatus) -> Style {
    match status {
        JobStatus::Running => running(),
        JobStatus::Succeeded => healthy(),
        JobStatus::Failed => failed(),
        JobStatus::Unknown => muted(),
    }
}

const MB: i64 = 1_048_576;
const GB: i64 = 1_073_741_824;

/// Color style for general I/O byte values.
pub fn metric_bytes_style(bytes: i64) -> Style {
    if bytes <= 0 {
        Style::default()
    } else if bytes < 100 * MB {
        muted()
    } else if bytes < GB {
        healthy()
    } else if bytes < 10 * GB {
        warning()
    } else {
        critical()
    }
}

/// Color style for shuffle byte values (lower thresholds).
pub fn shuffle_bytes_style(bytes: i64) -> Style {
    if bytes <= 0 {
        Style::default()
    } else if bytes < 100 * MB {
        Style::default()
    } else if bytes < 500 * MB {
        warning()
    } else {
        critical()
    }
}

/// Color style for spill byte values.
pub fn spill_bytes_style(bytes: i64) -> Style {
    if bytes <= 0 {
        Style::default()
    } else if bytes < GB {
        warning()
    } else {
        critical()
    }
}
