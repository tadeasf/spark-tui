use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use super::orchestrator::{assemble_data_payload, poll_once};
use crate::config::Config;
use crate::fetch::client::{FetchError, SparkHttpClient};
use crate::fetch::databricks::{ClusterInfo, ClusterState, DatabricksClient, SparkuiProbeResult};
use crate::fetch::eventlog;
use crate::fetch::types::ClusterResources;
use crate::tui::{Action, DataPayload, DataSourceMode};

pub async fn run_poller(
    client: Arc<SparkHttpClient>,
    databricks: Arc<DatabricksClient>,
    config: Arc<Config>,
    tx: mpsc::UnboundedSender<Action>,
    poll_interval: Duration,
) {
    // Discover app ID first
    let app_id = match client.discover_app_id().await {
        Ok(id) => {
            info!("Discovered Spark application: {}", id);
            id
        }
        Err(ref e) if is_cluster_unreachable(e) => {
            // Cluster may be terminated — check state and try historical
            handle_unreachable(&databricks, &config, &tx).await;
            return;
        }
        Err(e) => {
            error!("Failed to discover app ID: {}", e);
            let _ = tx.send(Action::FetchError(e));
            return;
        }
    };

    // Live poll loop
    loop {
        match poll_once(&client, &app_id).await {
            Ok(payload) => {
                if tx.send(Action::DataUpdate(Box::new(payload))).is_err() {
                    break;
                }
            }
            Err(ref e) if is_cluster_unreachable(e) => {
                warn!("Lost connection to cluster, checking state...");
                let _ = tx.send(Action::StatusMessage(
                    "Connection lost. Checking cluster state...".to_string(),
                ));
                handle_unreachable(&databricks, &config, &tx).await;
                return;
            }
            Err(e) => {
                warn!("Fetch error: {}", e);
                if tx.send(Action::FetchError(e)).is_err() {
                    break;
                }
            }
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// Check whether the error indicates the cluster is unreachable or terminated.
///
/// Databricks driver proxy returns different errors depending on cluster state:
/// - **503**: cluster not reachable (generic)
/// - **400 INVALID_STATE**: cluster is terminated/not running
/// - Connection error: cluster endpoint unreachable
fn is_cluster_unreachable(err: &FetchError) -> bool {
    match err {
        FetchError::ServiceUnavailable | FetchError::Request(_) => true,
        FetchError::HttpError { body, .. } => {
            body.contains("INVALID_STATE") || body.contains("TERMINATED")
        }
        _ => false,
    }
}

/// Handle a cluster that appears unreachable: check state, try historical data.
async fn handle_unreachable(
    databricks: &DatabricksClient,
    config: &Config,
    tx: &mpsc::UnboundedSender<Action>,
) {
    let cluster_info = match databricks.get_cluster_info(&config.cluster_id).await {
        Ok(info) => info,
        Err(e) => {
            warn!("Failed to check cluster state: {}", e);
            let _ = tx.send(Action::FetchError(FetchError::ServiceUnavailable));
            return;
        }
    };

    match cluster_info.state {
        ClusterState::Terminated => {
            let _ = tx.send(Action::StatusMessage(
                "Cluster terminated. Loading historical data...".to_string(),
            ));
            try_load_historical(databricks, config, &cluster_info, tx).await;
        }
        ClusterState::Pending | ClusterState::Restarting => {
            let msg = format!("Cluster is {}. Waiting...", cluster_info.state);
            let _ = tx.send(Action::StatusMessage(msg));
        }
        _ => {
            let msg = format!(
                "Cluster state: {}. Cannot connect to Spark UI.",
                cluster_info.state
            );
            let _ = tx.send(Action::FetchError(FetchError::ClusterTerminated(msg)));
        }
    }
}

/// Attempt to load historical Spark data using multiple strategies:
/// 0. Spark UI REST API (`/sparkui/{cluster}/{driver}/api/v1/`, needs `spark_context_id`)
/// 1. Spark History Server proxy API (Databricks internal)
/// 2. DBFS event logs via cluster_log_conf
/// 3. DBFS event logs at well-known default paths
async fn try_load_historical(
    databricks: &DatabricksClient,
    config: &Config,
    cluster_info: &ClusterInfo,
    tx: &mpsc::UnboundedSender<Action>,
) {
    // Strategy 0: Spark UI REST API (highest priority)
    if let Some(spark_ctx_id) = cluster_info.spark_context_id {
        let _ = tx.send(Action::StatusMessage(
            "Trying Historical Spark UI endpoint...".to_string(),
        ));
        if let Some(payload) = try_sparkui(databricks, config, spark_ctx_id, tx).await {
            let _ = tx.send(Action::DataUpdate(Box::new(payload)));
            return;
        }
    } else {
        warn!("No spark_context_id available; skipping sparkui strategy");
    }

    // Strategy 1: Spark History Server proxy
    let _ = tx.send(Action::StatusMessage(
        "Trying Spark History Server...".to_string(),
    ));

    if let Some(payload) = try_history_server(databricks, config).await {
        let _ = tx.send(Action::DataUpdate(Box::new(payload)));
        return;
    }

    // Strategy 2: DBFS event logs (cluster_log_conf or --event-log-path)
    let _ = tx.send(Action::StatusMessage(
        "Trying DBFS event logs...".to_string(),
    ));

    if let Some(payload) =
        try_dbfs_event_logs(databricks, config, cluster_info.log_conf.as_ref()).await
    {
        let _ = tx.send(Action::DataUpdate(Box::new(payload)));
        return;
    }

    // Strategy 3: Scan well-known DBFS default paths
    let _ = tx.send(Action::StatusMessage(
        "Scanning default DBFS paths...".to_string(),
    ));

    if let Some(path) = databricks.find_default_event_logs(&config.cluster_id).await {
        warn!("Found event log at default DBFS path: {}", path);
        if let Some(payload) = try_parse_dbfs_file(databricks, config, &path).await {
            let _ = tx.send(Action::DataUpdate(Box::new(payload)));
            return;
        }
    }

    // All strategies failed
    let _ = tx.send(Action::FetchError(FetchError::EventLogError(
        "Could not load historical data. Tried: Spark UI endpoint, \
         Spark History Server proxy, cluster log delivery config, \
         default DBFS paths. Configure cluster log delivery, \
         use --event-log-path, or try --sparkui-cookie."
            .to_string(),
    )));
}

/// Backoff delays (seconds) for retrying a Spark UI that is still loading.
const SPARKUI_RETRY_DELAYS: &[u64] = &[3, 5, 10, 15, 20];

/// Strategy 0: Try the Historical Spark UI REST API endpoint.
///
/// If the Spark UI returns a loading page (warm-up phase while downloading event logs),
/// retries with backoff before giving up.
async fn try_sparkui(
    databricks: &DatabricksClient,
    config: &Config,
    spark_context_id: i64,
    tx: &mpsc::UnboundedSender<Action>,
) -> Option<DataPayload> {
    match databricks
        .try_sparkui_endpoint(&config.cluster_id, spark_context_id)
        .await
    {
        SparkuiProbeResult::Ready { base_url, app_id } => {
            return fetch_sparkui_data_and_assemble(databricks, &base_url, &app_id).await;
        }
        SparkuiProbeResult::Loading { base_url } => {
            warn!(
                "Spark UI at {} is loading, will retry with backoff",
                base_url
            );
            for (attempt, delay) in SPARKUI_RETRY_DELAYS.iter().enumerate() {
                let _ = tx.send(Action::StatusMessage(format!(
                    "Spark UI loading... retrying ({}/{})",
                    attempt + 1,
                    SPARKUI_RETRY_DELAYS.len()
                )));
                tokio::time::sleep(Duration::from_secs(*delay)).await;

                match databricks.probe_sparkui_url(&base_url).await {
                    SparkuiProbeResult::Ready { base_url, app_id } => {
                        return fetch_sparkui_data_and_assemble(databricks, &base_url, &app_id)
                            .await;
                    }
                    SparkuiProbeResult::Loading { .. } => continue,
                    SparkuiProbeResult::NotFound => break,
                }
            }
            warn!(
                "Spark UI at {} did not become ready after {} retries",
                base_url,
                SPARKUI_RETRY_DELAYS.len()
            );
        }
        SparkuiProbeResult::NotFound => {}
    }

    None
}

/// Fetch full data from a ready Spark UI endpoint and assemble into a `DataPayload`.
async fn fetch_sparkui_data_and_assemble(
    databricks: &DatabricksClient,
    base_url: &str,
    app_id: &str,
) -> Option<DataPayload> {
    warn!("Using Historical Spark UI at {}", base_url);
    let data = match databricks.fetch_sparkui_data(base_url, app_id).await {
        Ok(d) => d,
        Err(e) => {
            warn!("Failed to fetch from sparkui endpoint: {}", e);
            return None;
        }
    };

    warn!(
        "Fetched via sparkui: {} jobs, {} stages",
        data.jobs.len(),
        data.stages.len()
    );

    assemble_data_payload(
        &data.app_id,
        data.jobs,
        data.stages,
        data.sql_executions,
        ClusterResources::default(),
        data.tasks,
        DataSourceMode::Historical,
        None,
        Some("Spark UI".to_string()),
    )
    .await
    .map_err(|e| warn!("Failed to assemble sparkui data: {}", e))
    .ok()
}

/// Strategy 1: Try Spark History Server proxy API.
async fn try_history_server(databricks: &DatabricksClient, config: &Config) -> Option<DataPayload> {
    let (base_path, app_id) = databricks
        .discover_history_server(&config.cluster_id)
        .await?;

    warn!("Using Spark History Server at {}", base_path);
    let data = match databricks.fetch_history_data(&base_path, &app_id).await {
        Ok(d) => d,
        Err(e) => {
            warn!("Failed to fetch from history server: {}", e);
            return None;
        }
    };

    warn!(
        "Fetched via history server: {} jobs, {} stages",
        data.jobs.len(),
        data.stages.len()
    );

    assemble_data_payload(
        &data.app_id,
        data.jobs,
        data.stages,
        data.sql_executions,
        ClusterResources::default(),
        data.tasks,
        DataSourceMode::Historical,
        None,
        Some("history server".to_string()),
    )
    .await
    .map_err(|e| warn!("Failed to assemble history server data: {}", e))
    .ok()
}

/// Strategy 2: DBFS event logs via cluster_log_conf or --event-log-path.
async fn try_dbfs_event_logs(
    databricks: &DatabricksClient,
    config: &Config,
    _log_conf: Option<&crate::fetch::databricks::LogDestination>,
) -> Option<DataPayload> {
    let parsed = match eventlog::load_event_log(
        databricks,
        &config.cluster_id,
        config.event_log_path.as_deref(),
    )
    .await
    {
        Ok(p) => p,
        Err(e) => {
            warn!("DBFS event log strategy failed: {}", e);
            return None;
        }
    };

    warn!(
        "Loaded from DBFS event log: {} jobs, {} stages",
        parsed.jobs.len(),
        parsed.stages.len()
    );

    let cluster_resources = parsed.cluster_resources.clone();
    assemble_data_payload(
        &parsed.app_id,
        parsed.jobs,
        parsed.stages,
        parsed.sql_executions,
        cluster_resources,
        parsed.tasks,
        DataSourceMode::Historical,
        None,
        Some("event logs".to_string()),
    )
    .await
    .map_err(|e| warn!("Failed to assemble event log data: {}", e))
    .ok()
}

/// Parse a specific DBFS file as an event log.
async fn try_parse_dbfs_file(
    databricks: &DatabricksClient,
    _config: &Config,
    path: &str,
) -> Option<DataPayload> {
    let parsed = match eventlog::load_event_log(databricks, "", Some(path)).await {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to parse DBFS file {}: {}", path, e);
            return None;
        }
    };

    warn!(
        "Loaded from default DBFS path: {} jobs, {} stages",
        parsed.jobs.len(),
        parsed.stages.len()
    );

    let cluster_resources = parsed.cluster_resources.clone();
    assemble_data_payload(
        &parsed.app_id,
        parsed.jobs,
        parsed.stages,
        parsed.sql_executions,
        cluster_resources,
        parsed.tasks,
        DataSourceMode::Historical,
        None,
        Some("event logs".to_string()),
    )
    .await
    .map_err(|e| warn!("Failed to assemble default path data: {}", e))
    .ok()
}
