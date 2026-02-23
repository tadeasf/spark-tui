use std::fmt;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use reqwest::Client;
use serde::Deserialize;

use tracing::warn;

use super::client::FetchError;
use super::types::{SparkApplication, SparkJob, SparkSqlExecution, SparkStage, SparkTask};

/// Thin client for Databricks REST API `/api/2.0/*` endpoints and workspace-level requests.
pub struct DatabricksClient {
    client: Client,
    /// Base URL for `/api/2.0` endpoints (e.g. `https://{host}/api/2.0`).
    base_url: String,
    /// Workspace root URL without path suffix (e.g. `https://{host}`).
    workspace_root: String,
    token: String,
    /// Optional cookie for Spark UI authentication (DATAPLANE_DOMAIN_DBAUTH value).
    sparkui_cookie: Option<String>,
}

// -- Public types --

/// Result of probing a Spark UI endpoint for readiness.
#[derive(Debug)]
pub enum SparkuiProbeResult {
    /// Found a working sparkui endpoint with application data.
    Ready { base_url: String, app_id: String },
    /// Sparkui is authenticated but still loading event logs (warm-up phase).
    Loading { base_url: String },
    /// No sparkui endpoint found or accessible.
    NotFound,
}

/// Cluster lifecycle state reported by Databricks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClusterState {
    Running,
    Terminated,
    Pending,
    Restarting,
    Terminating,
    Error,
    Unknown(String),
}

impl fmt::Display for ClusterState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Running => write!(f, "RUNNING"),
            Self::Terminated => write!(f, "TERMINATED"),
            Self::Pending => write!(f, "PENDING"),
            Self::Restarting => write!(f, "RESTARTING"),
            Self::Terminating => write!(f, "TERMINATING"),
            Self::Error => write!(f, "ERROR"),
            Self::Unknown(s) => write!(f, "{}", s),
        }
    }
}

impl From<&str> for ClusterState {
    fn from(s: &str) -> Self {
        match s {
            "RUNNING" => Self::Running,
            "TERMINATED" => Self::Terminated,
            "PENDING" => Self::Pending,
            "RESTARTING" => Self::Restarting,
            "TERMINATING" => Self::Terminating,
            "ERROR" => Self::Error,
            other => Self::Unknown(other.to_string()),
        }
    }
}

/// Where Databricks delivers cluster event logs.
#[derive(Debug, Clone)]
pub enum LogDestination {
    Dbfs { path: String },
    S3 { destination: String },
    Unsupported(String),
}

/// Consolidated cluster info from a single `/clusters/get` call.
#[derive(Debug, Clone)]
pub struct ClusterInfo {
    pub state: ClusterState,
    pub log_conf: Option<LogDestination>,
    pub spark_context_id: Option<i64>,
}

/// Data fetched from the Spark History Server proxy.
pub struct HistoryData {
    pub app_id: String,
    pub jobs: Vec<SparkJob>,
    pub stages: Vec<SparkStage>,
    pub sql_executions: Vec<SparkSqlExecution>,
    pub tasks: std::collections::HashMap<i64, Vec<SparkTask>>,
}

/// Entry from DBFS list endpoint.
#[derive(Debug, Clone)]
pub struct DbfsFileInfo {
    pub path: String,
    pub is_dir: bool,
    pub file_size: i64,
}

// -- Internal serde response types --

#[derive(Deserialize)]
struct ClusterGetResponse {
    state: String,
    #[serde(default)]
    cluster_log_conf: Option<ClusterLogConf>,
    #[serde(default)]
    spark_context_id: Option<i64>,
}

#[derive(Deserialize)]
struct ClusterLogConf {
    #[serde(default)]
    dbfs: Option<LogDbfsConf>,
    #[serde(default)]
    s3: Option<LogS3Conf>,
}

#[derive(Deserialize)]
struct LogDbfsConf {
    destination: String,
}

#[derive(Deserialize)]
struct LogS3Conf {
    destination: String,
}

#[derive(Deserialize)]
struct DbfsListResponse {
    #[serde(default)]
    files: Vec<DbfsFileEntry>,
}

#[derive(Deserialize)]
struct DbfsFileEntry {
    path: String,
    is_dir: bool,
    #[serde(default)]
    file_size: i64,
}

#[derive(Deserialize)]
struct DbfsReadResponse {
    #[serde(default)]
    bytes_read: i64,
    data: String,
}

// -- Implementation --

impl DatabricksClient {
    pub fn new(
        base_url: String,
        workspace_root: String,
        token: String,
        sparkui_cookie: Option<String>,
    ) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to build HTTP client for Databricks API");

        Self {
            client,
            base_url,
            workspace_root,
            token,
            sparkui_cookie,
        }
    }

    /// Fetch cluster log delivery configuration, if any.
    pub async fn get_cluster_log_conf(
        &self,
        cluster_id: &str,
    ) -> Result<Option<LogDestination>, FetchError> {
        let resp: ClusterGetResponse = self
            .api_get(&format!("/clusters/get?cluster_id={}", cluster_id))
            .await?;

        let dest = resp.cluster_log_conf.map(|conf| {
            if let Some(dbfs) = conf.dbfs {
                LogDestination::Dbfs {
                    path: dbfs.destination,
                }
            } else if let Some(s3) = conf.s3 {
                LogDestination::S3 {
                    destination: s3.destination,
                }
            } else {
                LogDestination::Unsupported("unknown log destination type".to_string())
            }
        });

        Ok(dest)
    }

    /// Fetch cluster state, log config, and spark context ID in a single API call.
    pub async fn get_cluster_info(&self, cluster_id: &str) -> Result<ClusterInfo, FetchError> {
        let resp: ClusterGetResponse = self
            .api_get(&format!("/clusters/get?cluster_id={}", cluster_id))
            .await?;

        let state = ClusterState::from(resp.state.as_str());

        let log_conf = resp.cluster_log_conf.map(|conf| {
            if let Some(dbfs) = conf.dbfs {
                LogDestination::Dbfs {
                    path: dbfs.destination,
                }
            } else if let Some(s3) = conf.s3 {
                LogDestination::S3 {
                    destination: s3.destination,
                }
            } else {
                LogDestination::Unsupported("unknown log destination type".to_string())
            }
        });

        Ok(ClusterInfo {
            state,
            log_conf,
            spark_context_id: resp.spark_context_id,
        })
    }

    /// List files/directories under a DBFS path.
    pub async fn dbfs_list(&self, path: &str) -> Result<Vec<DbfsFileInfo>, FetchError> {
        let resp: DbfsListResponse = self.api_get(&format!("/dbfs/list?path={}", path)).await?;

        Ok(resp
            .files
            .into_iter()
            .map(|e| DbfsFileInfo {
                path: e.path,
                is_dir: e.is_dir,
                file_size: e.file_size,
            })
            .collect())
    }

    /// Read a complete DBFS file using chunked reads (1 MB per chunk, base64-decoded).
    pub async fn dbfs_read_full(&self, path: &str) -> Result<Vec<u8>, FetchError> {
        const CHUNK_SIZE: i64 = 1_048_576; // 1 MB
        let mut offset: i64 = 0;
        let mut buf = Vec::new();

        loop {
            let resp: DbfsReadResponse = self
                .api_get(&format!(
                    "/dbfs/read?path={}&offset={}&length={}",
                    path, offset, CHUNK_SIZE
                ))
                .await?;

            let decoded = BASE64.decode(&resp.data).map_err(|e| {
                FetchError::EventLogError(format!(
                    "Base64 decode error at offset {}: {}",
                    offset, e
                ))
            })?;

            let chunk_len = decoded.len() as i64;
            buf.extend_from_slice(&decoded);

            if resp.bytes_read < CHUNK_SIZE {
                break;
            }
            offset += chunk_len;
        }

        Ok(buf)
    }

    /// Try to discover a working Spark History Server proxy endpoint for a terminated cluster.
    ///
    /// Databricks internally stores event logs and serves them via a history server.
    /// The proxy URL varies by platform/version, so we probe several known patterns.
    /// Returns `(base_path, app_id)` if a working endpoint is found.
    pub async fn discover_history_server(&self, cluster_id: &str) -> Option<(String, String)> {
        // Known and suspected Databricks Spark History Server proxy URL patterns.
        // Different Databricks platforms (AWS/Azure/GCP) and versions may use different paths.
        let candidate_paths = [
            // Common patterns across platforms
            format!("/spark-history-server-proxy/{}/api/v1", cluster_id),
            format!("/spark-history-server/{}/api/v1", cluster_id),
            format!("/spark-ui/{}/api/v1", cluster_id),
            format!("/sparkui/{}/api/v1", cluster_id),
            // Without cluster_id in path (history server may list all apps)
            "/spark-history-server/api/v1".to_string(),
        ];

        for base_path in &candidate_paths {
            let apps_path = format!("{}/applications", base_path);
            warn!(
                "Probing history server: {}{}",
                self.workspace_root, apps_path
            );

            match self.root_get_raw(&apps_path).await {
                Ok((status, body)) => {
                    if status == 200 {
                        match serde_json::from_str::<Vec<SparkApplication>>(&body) {
                            Ok(apps) => {
                                if let Some(app) = apps.into_iter().next() {
                                    warn!(
                                        "Found historical app via history server: {} at {}",
                                        app.id, base_path
                                    );
                                    return Some((base_path.clone(), app.id));
                                }
                                warn!(
                                    "History server at {} responded OK but no apps found",
                                    base_path
                                );
                            }
                            Err(_) => {
                                // Might be HTML or non-JSON — log a snippet
                                let snippet: String = body.chars().take(200).collect();
                                warn!(
                                    "History server at {} returned non-JSON (200): {}",
                                    base_path, snippet
                                );
                            }
                        }
                    } else {
                        let snippet: String = body.chars().take(150).collect();
                        warn!(
                            "History server probe {} returned HTTP {}: {}",
                            base_path, status, snippet
                        );
                    }
                }
                Err(e) => {
                    warn!("History server probe {} failed: {}", base_path, e);
                }
            }
        }

        None
    }

    /// Try the Databricks Historical Spark UI REST API endpoint.
    ///
    /// The endpoint is at `https://{host}/sparkui/{cluster_id}/driver-{spark_context_id}/api/v1/`.
    /// Also tries the dataplane domain variant (`adb-` → `adb-dp-` prefix).
    /// Returns `Ready` with base_url and app_id on success, `Loading` if the UI is warming up,
    /// or `NotFound` if no endpoint is accessible.
    pub async fn try_sparkui_endpoint(
        &self,
        cluster_id: &str,
        spark_context_id: i64,
    ) -> SparkuiProbeResult {
        let driver_id = format!("driver-{}", spark_context_id);

        // Build candidate base URLs: workspace root + optional dataplane variant
        let mut candidate_roots = vec![self.workspace_root.clone()];
        if self.workspace_root.contains("://adb-") && !self.workspace_root.contains("://adb-dp-") {
            let dp_root = self.workspace_root.replace("://adb-", "://adb-dp-");
            candidate_roots.push(dp_root);
        }

        for root in &candidate_roots {
            let base_url = format!("{}/sparkui/{}/{}/api/v1", root, cluster_id, driver_id);
            let apps_path = "/applications";

            // Try Bearer auth first
            warn!(
                "Probing sparkui endpoint (Bearer): {}{}",
                base_url, apps_path
            );
            match self.custom_get_raw(&base_url, apps_path, false).await {
                Ok((200, body)) => {
                    let result = self.parse_sparkui_apps(&body, &base_url);
                    match &result {
                        SparkuiProbeResult::NotFound => {} // try next auth/root
                        _ => return result,
                    }
                }
                Ok((status, _)) => {
                    warn!(
                        "Sparkui Bearer probe returned HTTP {} for {}",
                        status, base_url
                    );
                }
                Err(e) => {
                    warn!("Sparkui Bearer probe failed for {}: {}", base_url, e);
                }
            }

            // Try cookie auth if available
            if self.sparkui_cookie.is_some() {
                warn!(
                    "Probing sparkui endpoint (cookie): {}{}",
                    base_url, apps_path
                );
                match self.custom_get_raw(&base_url, apps_path, true).await {
                    Ok((200, body)) => {
                        let result = self.parse_sparkui_apps(&body, &base_url);
                        match &result {
                            SparkuiProbeResult::NotFound => {} // try next root
                            _ => return result,
                        }
                    }
                    Ok((status, _)) => {
                        warn!(
                            "Sparkui cookie probe returned HTTP {} for {}",
                            status, base_url
                        );
                    }
                    Err(e) => {
                        warn!("Sparkui cookie probe failed for {}: {}", base_url, e);
                    }
                }
            }
        }

        SparkuiProbeResult::NotFound
    }

    /// Probe a single sparkui URL for readiness (used for retry after `Loading` state).
    ///
    /// Tries cookie auth first (since we know it worked for the initial probe),
    /// then falls back to Bearer auth.
    pub async fn probe_sparkui_url(&self, base_url: &str) -> SparkuiProbeResult {
        let apps_path = "/applications";

        // Try cookie auth first if available (most likely to work since
        // the initial probe that returned Loading used cookie auth)
        if self.sparkui_cookie.is_some() {
            match self.custom_get_raw(base_url, apps_path, true).await {
                Ok((200, body)) => {
                    let result = self.parse_sparkui_apps(&body, base_url);
                    match &result {
                        SparkuiProbeResult::NotFound => {} // try Bearer
                        _ => return result,
                    }
                }
                Ok((status, _)) => {
                    warn!(
                        "Sparkui retry probe (cookie) returned HTTP {} for {}",
                        status, base_url
                    );
                }
                Err(e) => {
                    warn!(
                        "Sparkui retry probe (cookie) failed for {}: {}",
                        base_url, e
                    );
                }
            }
        }

        // Try Bearer auth
        match self.custom_get_raw(base_url, apps_path, false).await {
            Ok((200, body)) => self.parse_sparkui_apps(&body, base_url),
            Ok((status, _)) => {
                warn!(
                    "Sparkui retry probe (Bearer) returned HTTP {} for {}",
                    status, base_url
                );
                SparkuiProbeResult::NotFound
            }
            Err(e) => {
                warn!(
                    "Sparkui retry probe (Bearer) failed for {}: {}",
                    base_url, e
                );
                SparkuiProbeResult::NotFound
            }
        }
    }

    /// Parse the `/applications` response from a sparkui endpoint.
    ///
    /// Returns `Ready` if valid JSON with apps, `Loading` if the response is an HTML
    /// loading/warm-up page, or `NotFound` for other non-JSON responses.
    fn parse_sparkui_apps(&self, body: &str, base_url: &str) -> SparkuiProbeResult {
        // Check for HTML loading page (Spark UI warming up / downloading event logs)
        if Self::is_loading_page(body) {
            warn!("Sparkui at {} returned loading page (warming up)", base_url);
            return SparkuiProbeResult::Loading {
                base_url: base_url.to_string(),
            };
        }

        match serde_json::from_str::<Vec<SparkApplication>>(body) {
            Ok(apps) => {
                if let Some(app) = apps.into_iter().next() {
                    warn!(
                        "Found historical app via sparkui: {} at {}",
                        app.id, base_url
                    );
                    SparkuiProbeResult::Ready {
                        base_url: base_url.to_string(),
                        app_id: app.id,
                    }
                } else {
                    warn!("Sparkui at {} responded OK but no apps found", base_url);
                    SparkuiProbeResult::NotFound
                }
            }
            Err(_) => {
                let snippet: String = body.chars().take(200).collect();
                warn!(
                    "Sparkui at {} returned non-JSON (200): {}",
                    base_url, snippet
                );
                SparkuiProbeResult::NotFound
            }
        }
    }

    /// Detect whether an HTTP response body is a Spark UI loading/warm-up page.
    ///
    /// The Historical Spark UI needs to download and parse event logs from DBFS
    /// before serving JSON. During this warm-up, it returns HTML with loading indicators.
    fn is_loading_page(body: &str) -> bool {
        let lower = body.to_lowercase();
        // Common patterns in Spark UI loading pages
        lower.contains("<title>loading</title>")
            || lower.contains("loading spark ui")
            || lower.contains("please wait")
            || (lower.contains("<html") && !body.starts_with('[') && !body.starts_with('{'))
    }

    /// Fetch all data from the Spark UI REST API using cookie auth.
    /// `base_url` is a full URL like `https://{host}/sparkui/{cluster}/{driver}/api/v1`.
    pub async fn fetch_sparkui_data(
        &self,
        base_url: &str,
        app_id: &str,
    ) -> Result<HistoryData, FetchError> {
        let jobs_url = format!("{}/applications/{}/jobs", base_url, app_id);
        let stages_url = format!("{}/applications/{}/stages", base_url, app_id);
        let sql_url = format!("{}/applications/{}/sql", base_url, app_id);

        let (jobs_res, stages_res, sql_res) = tokio::join!(
            self.custom_get::<Vec<SparkJob>>(&jobs_url),
            self.custom_get::<Vec<SparkStage>>(&stages_url),
            self.custom_get::<Vec<SparkSqlExecution>>(&sql_url),
        );

        let jobs = jobs_res?;
        let stages = stages_res?;
        let sql_executions = sql_res.unwrap_or_default();

        // Fetch tasks for all stages
        let mut tasks = std::collections::HashMap::new();
        for stage in &stages {
            let path = format!(
                "{}/applications/{}/stages/{}/{}/taskList",
                base_url, app_id, stage.stage_id, stage.attempt_id
            );
            match self.custom_get::<Vec<SparkTask>>(&path).await {
                Ok(t) => {
                    tasks.insert(stage.stage_id, t);
                }
                Err(e) => {
                    warn!(
                        "Failed to fetch tasks for stage {}/{} from sparkui: {}",
                        stage.stage_id, stage.attempt_id, e
                    );
                }
            }
        }

        Ok(HistoryData {
            app_id: app_id.to_string(),
            jobs,
            stages,
            sql_executions,
            tasks,
        })
    }

    /// List well-known DBFS paths where Spark event logs may reside.
    ///
    /// Even without `cluster_log_conf`, Databricks may store event logs at default paths.
    /// Returns the first path that contains event log files.
    pub async fn find_default_event_logs(&self, cluster_id: &str) -> Option<String> {
        let candidate_dirs = [
            format!("dbfs:/cluster-logs/{}/eventlog", cluster_id),
            format!("dbfs:/databricks/spark/eventLogs/{}", cluster_id),
            format!("dbfs:/databricks/driver/eventlogs/{}", cluster_id),
            "dbfs:/cluster-logs".to_string(),
            "dbfs:/databricks/spark/eventLogs".to_string(),
        ];

        for dir in &candidate_dirs {
            warn!("Checking DBFS path for event logs: {}", dir);
            match self.dbfs_list(dir).await {
                Ok(files) => {
                    let log_files: Vec<_> = files.iter().filter(|f| !f.is_dir).collect();
                    if !log_files.is_empty() {
                        warn!("Found {} files at {}", log_files.len(), dir);
                        // Return the largest file
                        let mut sorted = log_files;
                        sorted.sort_by(|a, b| b.file_size.cmp(&a.file_size));
                        return Some(sorted[0].path.clone());
                    }
                    // Check subdirectories
                    let dirs: Vec<_> = files.iter().filter(|f| f.is_dir).collect();
                    for sub in dirs.iter().take(5) {
                        warn!("  Checking subdir: {}", sub.path);
                        if let Ok(sub_files) = self.dbfs_list(&sub.path).await {
                            let sub_logs: Vec<_> = sub_files.iter().filter(|f| !f.is_dir).collect();
                            if !sub_logs.is_empty() {
                                warn!("  Found {} files in {}", sub_logs.len(), sub.path);
                                let mut sorted = sub_logs;
                                sorted.sort_by(|a, b| b.file_size.cmp(&a.file_size));
                                return Some(sorted[0].path.clone());
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("DBFS path {} not accessible: {}", dir, e);
                }
            }
        }

        None
    }

    /// Fetch all data from the Spark History Server for a terminated cluster.
    /// Uses the same REST API shape as the live Spark driver.
    pub async fn fetch_history_data(
        &self,
        base_path: &str,
        app_id: &str,
    ) -> Result<HistoryData, FetchError> {
        let jobs_url = format!("{}/applications/{}/jobs", base_path, app_id);
        let stages_url = format!("{}/applications/{}/stages", base_path, app_id);
        let sql_url = format!("{}/applications/{}/sql", base_path, app_id);

        let (jobs_res, stages_res, sql_res) = tokio::join!(
            self.api_get::<Vec<SparkJob>>(&jobs_url),
            self.api_get::<Vec<SparkStage>>(&stages_url),
            self.api_get::<Vec<SparkSqlExecution>>(&sql_url),
        );

        let jobs = jobs_res?;
        let stages = stages_res?;
        let sql_executions = sql_res.unwrap_or_default();

        // Fetch tasks for all stages (historical = complete data)
        let mut tasks = std::collections::HashMap::new();
        for stage in &stages {
            let path = format!(
                "{}/applications/{}/stages/{}/{}/taskList",
                base_path, app_id, stage.stage_id, stage.attempt_id
            );
            match self.api_get::<Vec<SparkTask>>(&path).await {
                Ok(t) => {
                    tasks.insert(stage.stage_id, t);
                }
                Err(e) => {
                    warn!(
                        "Failed to fetch tasks for stage {}/{}: {}",
                        stage.stage_id, stage.attempt_id, e
                    );
                }
            }
        }

        Ok(HistoryData {
            app_id: app_id.to_string(),
            jobs,
            stages,
            sql_executions,
            tasks,
        })
    }

    /// GET against the workspace root (no `/api/2.0` prefix) with Bearer auth.
    async fn root_get_raw(&self, path: &str) -> Result<(u16, String), FetchError> {
        let url = format!("{}{}", self.workspace_root, path);
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read response body".into());
        Ok((status, body))
    }

    /// GET against an arbitrary base URL with optional cookie auth.
    /// Used for sparkui endpoints that may require cookie-based authentication.
    async fn custom_get_raw(
        &self,
        base_url: &str,
        path: &str,
        use_cookie: bool,
    ) -> Result<(u16, String), FetchError> {
        let url = format!("{}{}", base_url, path);
        let no_redirect_client = Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("Failed to build no-redirect HTTP client");

        let mut request = no_redirect_client.get(&url);

        if use_cookie {
            if let Some(cookie) = &self.sparkui_cookie {
                request = request.header("Cookie", format!("DATAPLANE_DOMAIN_DBAUTH={}", cookie));
            }
        } else {
            request = request.header("Authorization", format!("Bearer {}", self.token));
        }

        let response = request.send().await?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read response body".into());
        Ok((status, body))
    }

    /// GET against an arbitrary full URL with cookie auth, deserializing JSON response.
    async fn custom_get<T: serde::de::DeserializeOwned>(
        &self,
        full_url: &str,
    ) -> Result<T, FetchError> {
        let mut request = self.client.get(full_url);

        if let Some(cookie) = &self.sparkui_cookie {
            request = request.header("Cookie", format!("DATAPLANE_DOMAIN_DBAUTH={}", cookie));
        } else {
            request = request.header("Authorization", format!("Bearer {}", self.token));
        }

        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            return match status.as_u16() {
                401 => Err(FetchError::Unauthorized),
                403 => Err(FetchError::Forbidden),
                404 => Err(FetchError::NotFound),
                503 => Err(FetchError::ServiceUnavailable),
                code => {
                    let body = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unable to read response body".into());
                    Err(FetchError::HttpError { status: code, body })
                }
            };
        }

        let body = response.text().await?;
        serde_json::from_str::<T>(&body).map_err(|e| {
            let sample = if body.len() > 500 {
                format!("{}...(truncated)", &body[..500])
            } else {
                body
            };
            FetchError::Deserialize {
                source: e,
                url: full_url.to_string(),
                body_sample: sample,
            }
        })
    }

    /// Generic authenticated GET against the Databricks REST API.
    async fn api_get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, FetchError> {
        let url = format!("{}{}", self.base_url, path);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            return match status.as_u16() {
                401 => Err(FetchError::Unauthorized),
                403 => Err(FetchError::Forbidden),
                404 => Err(FetchError::NotFound),
                503 => Err(FetchError::ServiceUnavailable),
                code => {
                    let body = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unable to read response body".into());
                    Err(FetchError::HttpError { status: code, body })
                }
            };
        }

        let body = response.text().await?;

        serde_json::from_str::<T>(&body).map_err(|e| {
            let sample = if body.len() > 500 {
                format!("{}...(truncated)", &body[..500])
            } else {
                body
            };
            FetchError::Deserialize {
                source: e,
                url,
                body_sample: sample,
            }
        })
    }
}
