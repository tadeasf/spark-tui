use reqwest::Client;
use serde::de::DeserializeOwned;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FetchError {
    #[error(
        "Unauthorized (401): Token expired or invalid. Regenerate at Settings > Developer > Access Tokens."
    )]
    Unauthorized,

    #[error("Forbidden (403): Insufficient permissions to access this cluster.")]
    Forbidden,

    #[error("Not Found (404): Spark UI not available. Application may have ended.")]
    NotFound,

    #[error("Service Unavailable (503): Cluster not reachable. Is it running?")]
    ServiceUnavailable,

    #[error("HTTP {status}: {body}")]
    HttpError { status: u16, body: String },

    #[error("Deserialization error: {source}")]
    Deserialize {
        #[source]
        source: reqwest::Error,
        url: String,
    },

    #[error("Request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("No Spark applications found on the cluster.")]
    NoApplications,
}

pub struct SparkHttpClient {
    client: Client,
    base_url: String,
    token: String,
}

impl SparkHttpClient {
    pub fn new(base_url: String, token: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            base_url,
            token,
        }
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, FetchError> {
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

        response
            .json::<T>()
            .await
            .map_err(|e| FetchError::Deserialize { source: e, url })
    }
}
