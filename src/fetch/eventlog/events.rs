use std::collections::HashMap;

use serde::Deserialize;

/// Top-level Spark event log event, dispatched by `Event` tag.
#[derive(Debug, Deserialize)]
#[serde(tag = "Event")]
#[allow(dead_code)]
pub enum SparkEvent {
    #[serde(rename = "SparkListenerApplicationStart")]
    ApplicationStart {
        #[serde(rename = "App ID")]
        app_id: Option<String>,
        #[serde(rename = "App Name")]
        app_name: Option<String>,
        #[serde(rename = "Timestamp")]
        timestamp: Option<i64>,
    },

    #[serde(rename = "SparkListenerApplicationEnd")]
    ApplicationEnd {
        #[serde(rename = "Timestamp")]
        timestamp: Option<i64>,
    },

    #[serde(rename = "SparkListenerJobStart")]
    JobStart {
        #[serde(rename = "Job ID")]
        job_id: i64,
        #[serde(rename = "Submission Time")]
        submission_time: Option<i64>,
        #[serde(rename = "Stage IDs")]
        stage_ids: Vec<i64>,
        #[serde(rename = "Stage Infos", default)]
        stage_infos: Vec<EventStageInfo>,
        #[serde(rename = "Properties", default)]
        properties: HashMap<String, String>,
    },

    #[serde(rename = "SparkListenerJobEnd")]
    JobEnd {
        #[serde(rename = "Job ID")]
        job_id: i64,
        #[serde(rename = "Completion Time")]
        completion_time: Option<i64>,
        #[serde(rename = "Job Result")]
        job_result: Option<EventJobResult>,
    },

    #[serde(rename = "SparkListenerStageSubmitted")]
    StageSubmitted {
        #[serde(rename = "Stage Info")]
        stage_info: EventStageInfo,
    },

    #[serde(rename = "SparkListenerStageCompleted")]
    StageCompleted {
        #[serde(rename = "Stage Info")]
        stage_info: EventStageInfo,
    },

    #[serde(rename = "SparkListenerTaskEnd")]
    TaskEnd {
        #[serde(rename = "Stage ID")]
        stage_id: i64,
        #[serde(rename = "Stage Attempt ID")]
        stage_attempt_id: i64,
        #[serde(rename = "Task Info")]
        task_info: Option<EventTaskInfo>,
        #[serde(rename = "Task Metrics")]
        task_metrics: Option<EventTaskMetrics>,
        #[serde(rename = "Task End Reason")]
        task_end_reason: Option<EventTaskEndReason>,
    },

    #[serde(rename = "SparkListenerExecutorAdded")]
    ExecutorAdded {
        #[serde(rename = "Executor ID")]
        executor_id: String,
        #[serde(rename = "Executor Info")]
        executor_info: Option<EventExecutorInfo>,
    },

    #[serde(rename = "SparkListenerExecutorRemoved")]
    ExecutorRemoved {
        #[serde(rename = "Executor ID")]
        executor_id: String,
    },

    #[serde(rename = "org.apache.spark.sql.execution.ui.SparkListenerSQLExecutionStart")]
    SqlExecutionStart {
        #[serde(rename = "executionId")]
        execution_id: i64,
        #[serde(default)]
        description: Option<String>,
        #[serde(rename = "physicalPlanDescription", default)]
        physical_plan: Option<String>,
        #[serde(default)]
        time: Option<i64>,
    },

    #[serde(rename = "org.apache.spark.sql.execution.ui.SparkListenerSQLExecutionEnd")]
    SqlExecutionEnd {
        #[serde(rename = "executionId")]
        execution_id: i64,
        #[serde(default)]
        time: Option<i64>,
    },

    /// Catch-all for events we don't parse.
    #[serde(other)]
    Unknown,
}

// -- Sub-types --

#[derive(Debug, Clone, Deserialize)]
pub struct EventStageInfo {
    #[serde(rename = "Stage ID")]
    pub stage_id: i64,
    #[serde(rename = "Stage Attempt ID", default)]
    pub stage_attempt_id: i64,
    #[serde(rename = "Stage Name", default)]
    pub stage_name: String,
    #[serde(rename = "Number of Tasks", default)]
    pub num_tasks: i64,
    #[serde(rename = "Submission Time", default)]
    pub submission_time: Option<i64>,
    #[serde(rename = "Completion Time", default)]
    pub completion_time: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventTaskInfo {
    #[serde(rename = "Task ID")]
    pub task_id: i64,
    #[serde(rename = "Index", default)]
    pub index: i64,
    #[serde(rename = "Attempt", default)]
    pub attempt: i64,
    #[serde(rename = "Executor ID", default)]
    pub executor_id: String,
    #[serde(rename = "Host", default)]
    pub host: String,
    #[serde(rename = "Launch Time", default)]
    pub launch_time: Option<i64>,
    #[serde(rename = "Finish Time", default)]
    pub finish_time: Option<i64>,
    #[serde(rename = "Failed", default)]
    pub failed: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventTaskMetrics {
    #[serde(rename = "Executor Run Time", default)]
    pub executor_run_time: i64,
    #[serde(rename = "Executor CPU Time", default)]
    pub executor_cpu_time: i64,
    #[serde(rename = "Memory Bytes Spilled", default)]
    pub memory_bytes_spilled: i64,
    #[serde(rename = "Disk Bytes Spilled", default)]
    pub disk_bytes_spilled: i64,
    #[serde(rename = "Peak Execution Memory", default)]
    pub peak_execution_memory: i64,
    #[serde(rename = "Input Metrics", default)]
    pub input_metrics: Option<EventInputMetrics>,
    #[serde(rename = "Output Metrics", default)]
    pub output_metrics: Option<EventOutputMetrics>,
    #[serde(rename = "Shuffle Read Metrics", default)]
    pub shuffle_read_metrics: Option<EventShuffleReadMetrics>,
    #[serde(rename = "Shuffle Write Metrics", default)]
    pub shuffle_write_metrics: Option<EventShuffleWriteMetrics>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventInputMetrics {
    #[serde(rename = "Bytes Read", default)]
    pub bytes_read: i64,
    #[serde(rename = "Records Read", default)]
    pub records_read: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventOutputMetrics {
    #[serde(rename = "Bytes Written", default)]
    pub bytes_written: i64,
    #[serde(rename = "Records Written", default)]
    pub records_written: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventShuffleReadMetrics {
    #[serde(rename = "Remote Bytes Read", default)]
    pub remote_bytes_read: i64,
    #[serde(rename = "Local Bytes Read", default)]
    pub local_bytes_read: i64,
    #[serde(rename = "Total Records Read", default)]
    pub records_read: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventShuffleWriteMetrics {
    #[serde(rename = "Shuffle Bytes Written", default)]
    pub bytes_written: i64,
    #[serde(rename = "Shuffle Records Written", default)]
    pub records_written: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventJobResult {
    #[serde(rename = "Result", default)]
    pub result: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct EventExecutorInfo {
    #[serde(rename = "Total Cores", default)]
    pub total_cores: i64,
    #[serde(rename = "Host", default)]
    pub host: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventTaskEndReason {
    #[serde(rename = "Reason", default)]
    pub reason: String,
}
