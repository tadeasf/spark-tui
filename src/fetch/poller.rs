use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use super::orchestrator::poll_once;
use crate::fetch::client::SparkHttpClient;
use crate::tui::Action;

pub async fn run_poller(
    client: Arc<SparkHttpClient>,
    tx: mpsc::UnboundedSender<Action>,
    poll_interval: Duration,
) {
    // Discover app ID first
    let app_id = match client.discover_app_id().await {
        Ok(id) => {
            info!("Discovered Spark application: {}", id);
            id
        }
        Err(e) => {
            error!("Failed to discover app ID: {}", e);
            let _ = tx.send(Action::FetchError(e));
            return;
        }
    };

    loop {
        match poll_once(&client, &app_id).await {
            Ok(payload) => {
                if tx.send(Action::DataUpdate(Box::new(payload))).is_err() {
                    break; // receiver dropped
                }
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
