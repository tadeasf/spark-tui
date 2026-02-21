use super::client::{FetchError, SparkHttpClient};
use super::types::*;

impl SparkHttpClient {
    pub async fn discover_app_id(&self) -> Result<String, FetchError> {
        let apps: Vec<SparkApplication> = self.get("/applications").await?;
        apps.into_iter()
            .next()
            .map(|a| a.id)
            .ok_or(FetchError::NoApplications)
    }

    pub async fn get_jobs(&self, app_id: &str) -> Result<Vec<SparkJob>, FetchError> {
        self.get(&format!("/applications/{}/jobs", app_id)).await
    }

    pub async fn get_stages(&self, app_id: &str) -> Result<Vec<SparkStage>, FetchError> {
        self.get(&format!("/applications/{}/stages", app_id)).await
    }

    pub async fn get_sql_executions(
        &self,
        app_id: &str,
    ) -> Result<Vec<SparkSqlExecution>, FetchError> {
        self.get(&format!("/applications/{}/sql", app_id)).await
    }

    pub async fn get_task_list(
        &self,
        app_id: &str,
        stage_id: i64,
        attempt_id: i64,
    ) -> Result<Vec<SparkTask>, FetchError> {
        self.get(&format!(
            "/applications/{}/stages/{}/{}/taskList",
            app_id, stage_id, attempt_id
        ))
        .await
    }

    pub async fn get_executors(&self, app_id: &str) -> Result<Vec<SparkExecutor>, FetchError> {
        self.get(&format!("/applications/{}/executors", app_id))
            .await
    }
}
