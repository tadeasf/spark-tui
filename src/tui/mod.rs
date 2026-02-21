pub mod app;
pub mod highlight;
pub mod tabs;
pub mod theme;
pub mod widgets;

use std::collections::HashMap;
use std::sync::Arc;

use crate::analyze::types::{HealthSummary, RankedJob, Suspect};
use crate::fetch::client::FetchError;
use crate::fetch::types::{SparkSqlExecution, SparkStage, SparkTask};
use crossterm::event::KeyEvent;

/// All data needed to render the TUI.
#[derive(Debug, Clone)]
pub struct DataPayload {
    pub app_id: String,
    pub jobs: Vec<RankedJob>,
    pub stages: Vec<SparkStage>,
    pub sql_executions: Vec<SparkSqlExecution>,
    pub suspects: Vec<Suspect>,
    pub stage_tasks: Arc<HashMap<i64, Vec<SparkTask>>>,
    pub summary: HealthSummary,
    pub last_updated: String,
}

/// Actions sent through the channel to the app event loop.
#[allow(dead_code)]
pub enum Action {
    Key(KeyEvent),
    Mouse(crossterm::event::MouseEvent),
    Resize(u16, u16),
    DataUpdate(DataPayload),
    FetchError(FetchError),
}
