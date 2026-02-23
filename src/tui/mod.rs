pub mod app;
pub mod highlight;
pub mod tabs;
pub mod theme;
pub mod widgets;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::analyze::types::{HealthSummary, RankedJob, Suspect};
use crate::fetch::client::FetchError;
use crate::fetch::types::{ClusterResources, SparkSqlExecution, SparkStage, SparkTask};
use crossterm::event::KeyEvent;

/// Whether data comes from the live Spark REST API or parsed historical event logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSourceMode {
    Live,
    Historical,
}

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
    pub cluster_resources: ClusterResources,
    pub stage_sql_hints: Arc<HashMap<i64, String>>,
    pub critical_stages: Arc<HashSet<i64>>,
    pub last_updated: String,
    pub data_source: DataSourceMode,
    /// Optional detail about the data source (e.g. "Spark UI", "event logs").
    pub data_source_detail: Option<String>,
}

/// Actions sent through the channel to the app event loop.
#[allow(dead_code)]
pub enum Action {
    Key(KeyEvent),
    Mouse(crossterm::event::MouseEvent),
    Resize(u16, u16),
    DataUpdate(Box<DataPayload>),
    FetchError(FetchError),
    TaskDataLoaded(i64, Vec<SparkTask>),
    TaskFetchFailed(i64, String),
    StatusMessage(String),
}
