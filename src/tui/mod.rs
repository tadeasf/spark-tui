pub mod app;
pub mod highlight;
pub mod tabs;
pub mod theme;
pub mod widgets;

use crate::analyze::types::{RankedJob, Suspect};
use crate::fetch::client::FetchError;
use crate::fetch::types::{SparkSqlExecution, SparkStage};
use crossterm::event::{KeyEvent, MouseEvent};

/// All data needed to render the TUI.
#[derive(Debug, Clone)]
pub struct DataPayload {
    pub app_id: String,
    pub jobs: Vec<RankedJob>,
    pub stages: Vec<SparkStage>,
    pub sql_executions: Vec<SparkSqlExecution>,
    pub suspects: Vec<Suspect>,
    pub last_updated: String,
}

/// Actions sent through the channel to the app event loop.
pub enum Action {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    DataUpdate(DataPayload),
    FetchError(FetchError),
    Tick,
}
