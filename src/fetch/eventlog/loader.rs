use std::io::{BufReader, Cursor};

use flate2::read::GzDecoder;
use tracing::info;

use super::parser::{EventLogParser, ParsedEventLog};
use crate::fetch::client::FetchError;
use crate::fetch::databricks::{DatabricksClient, LogDestination};

/// Load and parse a Spark event log from DBFS (auto-discovered or explicit path).
pub async fn load_event_log(
    databricks: &DatabricksClient,
    cluster_id: &str,
    explicit_path: Option<&str>,
) -> Result<ParsedEventLog, FetchError> {
    let path = match explicit_path {
        Some(p) => p.to_string(),
        None => discover_event_log_path(databricks, cluster_id).await?,
    };

    info!("Loading event log from: {}", path);

    let raw = databricks.dbfs_read_full(&path).await?;
    if raw.is_empty() {
        return Err(FetchError::EventLogError(
            "Event log file is empty".to_string(),
        ));
    }

    let parsed = parse_bytes(&raw, &path)?;

    if parsed.jobs.is_empty() && parsed.stages.is_empty() {
        return Err(FetchError::NoEventLogs);
    }

    Ok(parsed)
}

/// Discover the event log path from cluster log configuration.
async fn discover_event_log_path(
    databricks: &DatabricksClient,
    cluster_id: &str,
) -> Result<String, FetchError> {
    let log_conf = databricks.get_cluster_log_conf(cluster_id).await?;

    let base_path = match log_conf {
        Some(LogDestination::Dbfs { path }) => path,
        Some(LogDestination::S3 { destination }) => {
            return Err(FetchError::EventLogError(format!(
                "S3 log destination ({}) is not supported yet. \
                 Use --event-log-path with a DBFS path.",
                destination
            )));
        }
        Some(LogDestination::Unsupported(desc)) => {
            return Err(FetchError::EventLogError(format!(
                "Unsupported log destination: {}. Use --event-log-path.",
                desc
            )));
        }
        None => {
            return Err(FetchError::NoEventLogs);
        }
    };

    let base_path = base_path.trim_end_matches('/');
    let eventlog_dir = format!("{}/{}/eventlog", base_path, cluster_id);
    info!("Searching for event logs in: {}", eventlog_dir);

    let files = match databricks.dbfs_list(&eventlog_dir).await {
        Ok(f) => f,
        Err(FetchError::NotFound) => {
            return Err(FetchError::NoEventLogs);
        }
        Err(e) => return Err(e),
    };

    // Collect non-directory files
    let mut candidates: Vec<_> = files.iter().filter(|f| !f.is_dir).collect();

    if candidates.is_empty() {
        // Spark 3.x may place logs inside app-id subdirectories
        let dirs: Vec<_> = files.iter().filter(|f| f.is_dir).collect();
        for dir in dirs {
            if let Ok(sub_files) = databricks.dbfs_list(&dir.path).await {
                let mut sub_candidates: Vec<_> =
                    sub_files.into_iter().filter(|f| !f.is_dir).collect();
                if !sub_candidates.is_empty() {
                    sub_candidates.sort_by(|a, b| b.file_size.cmp(&a.file_size));
                    return Ok(sub_candidates.swap_remove(0).path);
                }
            }
        }
        return Err(FetchError::NoEventLogs);
    }

    // Pick the largest file (most likely the complete event log)
    candidates.sort_by(|a, b| b.file_size.cmp(&a.file_size));
    Ok(candidates[0].path.clone())
}

/// Parse raw bytes into a `ParsedEventLog`, handling gzip if needed.
fn parse_bytes(raw: &[u8], path: &str) -> Result<ParsedEventLog, FetchError> {
    let is_gzip = (raw.len() >= 2 && raw[0] == 0x1f && raw[1] == 0x8b)
        || path.ends_with(".gz")
        || path.ends_with(".gzip");

    let mut parser = EventLogParser::new();

    if is_gzip {
        info!("Decompressing gzip event log");
        let decoder = GzDecoder::new(Cursor::new(raw));
        let reader = BufReader::new(decoder);
        parser.process_reader(reader);
    } else {
        let reader = BufReader::new(Cursor::new(raw));
        parser.process_reader(reader);
    }

    Ok(parser.finish())
}
