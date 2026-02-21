use std::collections::HashSet;
use std::sync::Arc;

use ratatui::widgets::TableState;
use tokio::sync::mpsc;
use tui_scrollview::ScrollViewState;

use crate::fetch::client::SparkHttpClient;
use crate::tui::{Action, DataPayload};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Jobs,
    Suspects,
}

impl Tab {
    pub fn titles() -> Vec<&'static str> {
        vec!["Jobs", "Suspects"]
    }

    pub fn index(self) -> usize {
        match self {
            Tab::Jobs => 0,
            Tab::Suspects => 1,
        }
    }

    pub fn from_index(idx: usize) -> Self {
        match idx {
            0 => Tab::Jobs,
            1 => Tab::Suspects,
            _ => Tab::Jobs,
        }
    }

    pub fn next(self) -> Self {
        let len = Tab::titles().len();
        Tab::from_index((self.index() + 1) % len)
    }

    pub fn prev(self) -> Self {
        let len = Tab::titles().len();
        Tab::from_index((self.index() + len - 1) % len)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    List,
    JobDetail,
    SqlDetail,
    StageDetail,
}

pub struct App {
    pub(super) active_tab: Tab,
    pub(super) view_mode: ViewMode,
    pub(super) data: Option<Arc<DataPayload>>,
    pub(super) error_msg: Option<String>,
    pub(super) cluster_id: String,
    pub(super) should_quit: bool,
    pub(super) job_table_state: TableState,
    pub(super) suspect_table_state: TableState,
    pub(super) detail_table_state: TableState,
    pub(super) sql_scroll_state: ScrollViewState,
    pub(super) stage_detail_scroll_state: ScrollViewState,
    pub(super) show_help: bool,
    pub(super) return_tab: Option<Tab>,
    pub(super) client: Arc<SparkHttpClient>,
    pub(super) tx: mpsc::UnboundedSender<Action>,
    pub(super) pending_task_fetches: HashSet<i64>,
}

impl App {
    pub fn new(
        cluster_id: String,
        client: Arc<SparkHttpClient>,
        tx: mpsc::UnboundedSender<Action>,
    ) -> Self {
        Self {
            active_tab: Tab::Jobs,
            view_mode: ViewMode::List,
            data: None,
            error_msg: None,
            cluster_id,
            should_quit: false,
            job_table_state: TableState::default(),
            suspect_table_state: TableState::default(),
            detail_table_state: TableState::default(),
            sql_scroll_state: ScrollViewState::default(),
            stage_detail_scroll_state: ScrollViewState::default(),
            show_help: false,
            return_tab: None,
            client,
            tx,
            pending_task_fetches: HashSet::new(),
        }
    }

    pub(super) fn active_table_state(&mut self) -> Option<&mut TableState> {
        match self.view_mode {
            ViewMode::JobDetail => Some(&mut self.detail_table_state),
            ViewMode::List => match self.active_tab {
                Tab::Jobs => Some(&mut self.job_table_state),
                Tab::Suspects => Some(&mut self.suspect_table_state),
            },
            ViewMode::SqlDetail | ViewMode::StageDetail => None, // Scroll-based, no table
        }
    }
}
