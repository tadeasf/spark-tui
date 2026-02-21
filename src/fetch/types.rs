use serde::de::Deserializer;
use serde::Deserialize;

// -- Application --

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SparkApplication {
    pub id: String,
    pub name: String,
}

// -- Jobs --

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JobStatus {
    Running,
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SparkJob {
    pub job_id: i64,
    pub name: String,
    pub status: JobStatus,
    pub submission_time: Option<String>,
    pub completion_time: Option<String>,
    pub stage_ids: Vec<i64>,
    pub num_tasks: i64,
    pub num_active_tasks: i64,
    pub num_completed_tasks: i64,
    pub num_skipped_tasks: i64,
    pub num_failed_tasks: i64,
    pub num_killed_tasks: i64,
    pub num_completed_indices: i64,
    pub num_active_stages: i64,
    pub num_completed_stages: i64,
    pub num_skipped_stages: i64,
    pub num_failed_stages: i64,
}

// -- Stages --

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StageStatus {
    Active,
    Complete,
    Failed,
    Pending,
    Skipped,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SparkStage {
    pub status: StageStatus,
    pub stage_id: i64,
    pub attempt_id: i64,
    pub num_tasks: i64,
    pub num_active_tasks: i64,
    pub num_complete_tasks: i64,
    pub num_failed_tasks: i64,
    pub num_killed_tasks: i64,
    pub executor_run_time: i64,
    pub executor_cpu_time: i64,
    pub input_bytes: i64,
    pub input_records: i64,
    pub output_bytes: i64,
    pub output_records: i64,
    pub shuffle_read_bytes: i64,
    pub shuffle_read_records: i64,
    pub shuffle_write_bytes: i64,
    pub shuffle_write_records: i64,
    pub memory_bytes_spilled: i64,
    pub disk_bytes_spilled: i64,
    pub name: String,
    pub submission_time: Option<String>,
    pub completion_time: Option<String>,
    pub first_task_launched_time: Option<String>,
}

// -- SQL Executions --

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SparkSqlExecution {
    pub id: i64,
    pub status: String,
    pub description: String,
    pub plan_description: String,
    pub submission_time: String,
    #[serde(default)]
    pub duration: i64,
    #[serde(default)]
    pub running_job_ids: Vec<i64>,
    #[serde(default)]
    pub success_job_ids: Vec<i64>,
    #[serde(default)]
    pub failed_job_ids: Vec<i64>,
}

// -- Tasks --

/// Public task type with flat metric fields.
/// Deserialized from Spark's nested `taskMetrics` JSON via `RawSparkTask`.
#[derive(Debug, Clone)]
pub struct SparkTask {
    pub task_id: i64,
    pub index: i64,
    pub attempt: i64,
    pub stage_id: i64,
    pub stage_attempt_id: i64,
    pub executor_id: String,
    pub host: String,
    pub status: String,
    pub duration: Option<i64>,
    pub executor_run_time: i64,
    pub executor_cpu_time: i64,
    pub input_bytes: i64,
    pub input_records: i64,
    pub output_bytes: i64,
    pub output_records: i64,
    pub shuffle_read_bytes: i64,
    pub shuffle_read_records: i64,
    pub shuffle_write_bytes: i64,
    pub shuffle_write_records: i64,
    pub memory_bytes_spilled: i64,
    pub disk_bytes_spilled: i64,
}

// -- Raw serde types matching Spark REST API nested shape --

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawInputMetrics {
    #[serde(default)]
    bytes_read: i64,
    #[serde(default)]
    records_read: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOutputMetrics {
    #[serde(default)]
    bytes_written: i64,
    #[serde(default)]
    records_written: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawShuffleReadMetrics {
    #[serde(default)]
    remote_bytes_read: i64,
    #[serde(default)]
    local_bytes_read: i64,
    #[serde(default)]
    records_read: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawShuffleWriteMetrics {
    #[serde(default)]
    bytes_written: i64,
    #[serde(default)]
    records_written: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTaskMetrics {
    #[serde(default)]
    executor_run_time: i64,
    #[serde(default)]
    executor_cpu_time: i64,
    #[serde(default)]
    memory_bytes_spilled: i64,
    #[serde(default)]
    disk_bytes_spilled: i64,
    #[serde(default)]
    input_metrics: Option<RawInputMetrics>,
    #[serde(default)]
    output_metrics: Option<RawOutputMetrics>,
    #[serde(default)]
    shuffle_read_metrics: Option<RawShuffleReadMetrics>,
    #[serde(default)]
    shuffle_write_metrics: Option<RawShuffleWriteMetrics>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSparkTask {
    task_id: i64,
    index: i64,
    attempt: i64,
    stage_id: i64,
    stage_attempt_id: i64,
    executor_id: String,
    host: String,
    status: String,
    duration: Option<i64>,
    #[serde(default)]
    task_metrics: Option<RawTaskMetrics>,
}

impl<'de> Deserialize<'de> for SparkTask {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawSparkTask::deserialize(deserializer)?;
        let m = raw.task_metrics.unwrap_or(RawTaskMetrics {
            executor_run_time: 0,
            executor_cpu_time: 0,
            memory_bytes_spilled: 0,
            disk_bytes_spilled: 0,
            input_metrics: None,
            output_metrics: None,
            shuffle_read_metrics: None,
            shuffle_write_metrics: None,
        });

        let (input_bytes, input_records) = match &m.input_metrics {
            Some(im) => (im.bytes_read, im.records_read),
            None => (0, 0),
        };
        let (output_bytes, output_records) = match &m.output_metrics {
            Some(om) => (om.bytes_written, om.records_written),
            None => (0, 0),
        };
        let (shuffle_read_bytes, shuffle_read_records) = match &m.shuffle_read_metrics {
            Some(sr) => (sr.remote_bytes_read + sr.local_bytes_read, sr.records_read),
            None => (0, 0),
        };
        let (shuffle_write_bytes, shuffle_write_records) = match &m.shuffle_write_metrics {
            Some(sw) => (sw.bytes_written, sw.records_written),
            None => (0, 0),
        };

        Ok(SparkTask {
            task_id: raw.task_id,
            index: raw.index,
            attempt: raw.attempt,
            stage_id: raw.stage_id,
            stage_attempt_id: raw.stage_attempt_id,
            executor_id: raw.executor_id,
            host: raw.host,
            status: raw.status,
            duration: raw.duration,
            executor_run_time: m.executor_run_time,
            executor_cpu_time: m.executor_cpu_time,
            input_bytes,
            input_records,
            output_bytes,
            output_records,
            shuffle_read_bytes,
            shuffle_read_records,
            shuffle_write_bytes,
            shuffle_write_records,
            memory_bytes_spilled: m.memory_bytes_spilled,
            disk_bytes_spilled: m.disk_bytes_spilled,
        })
    }
}
