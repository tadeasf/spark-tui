use std::collections::HashMap;
use std::io::BufRead;

use tracing::warn;

use super::events::*;
use crate::fetch::types::{
    ClusterResources, JobStatus, SparkExecutor, SparkJob, SparkSqlExecution, SparkStage, SparkTask,
    StageStatus,
};

/// Final output of event log parsing, using the same types the live API returns.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ParsedEventLog {
    pub app_id: String,
    pub jobs: Vec<SparkJob>,
    pub stages: Vec<SparkStage>,
    pub tasks: HashMap<i64, Vec<SparkTask>>,
    pub executors: Vec<SparkExecutor>,
    pub sql_executions: Vec<SparkSqlExecution>,
    pub cluster_resources: ClusterResources,
}

/// Stateful accumulator that processes event log lines and builds the parsed result.
pub struct EventLogParser {
    app_id: Option<String>,
    jobs: HashMap<i64, JobAccumulator>,
    stages: HashMap<(i64, i64), StageAccumulator>,
    tasks: HashMap<i64, Vec<SparkTask>>,
    executors: HashMap<String, ExecutorAccumulator>,
    sql_executions: HashMap<i64, SqlAccumulator>,
    errors: usize,
}

struct JobAccumulator {
    job_id: i64,
    name: String,
    submission_time: Option<i64>,
    completion_time: Option<i64>,
    stage_ids: Vec<i64>,
    status: JobStatus,
    num_tasks: i64,
}

struct StageAccumulator {
    stage_id: i64,
    attempt_id: i64,
    name: String,
    num_tasks: i64,
    submission_time: Option<i64>,
    completion_time: Option<i64>,
    status: StageStatus,
    // Aggregated metrics (summed from tasks)
    executor_run_time: i64,
    executor_cpu_time: i64,
    input_bytes: i64,
    input_records: i64,
    output_bytes: i64,
    output_records: i64,
    shuffle_read_bytes: i64,
    shuffle_read_records: i64,
    shuffle_write_bytes: i64,
    shuffle_write_records: i64,
    memory_bytes_spilled: i64,
    disk_bytes_spilled: i64,
    peak_execution_memory: i64,
    completed_tasks: i64,
    failed_tasks: i64,
    killed_tasks: i64,
}

struct ExecutorAccumulator {
    id: String,
    total_cores: i64,
    is_active: bool,
}

struct SqlAccumulator {
    id: i64,
    description: String,
    plan_description: String,
    start_time: Option<i64>,
    end_time: Option<i64>,
    job_ids: Vec<i64>,
}

impl EventLogParser {
    pub fn new() -> Self {
        Self {
            app_id: None,
            jobs: HashMap::new(),
            stages: HashMap::new(),
            tasks: HashMap::new(),
            executors: HashMap::new(),
            sql_executions: HashMap::new(),
            errors: 0,
        }
    }

    /// Process all lines from a reader.
    pub fn process_reader<R: BufRead>(&mut self, reader: R) {
        for line in reader.lines() {
            match line {
                Ok(l) => self.process_line(&l),
                Err(e) => {
                    self.errors += 1;
                    warn!("Error reading event log line: {}", e);
                }
            }
        }
    }

    /// Parse a single JSON line and dispatch to the appropriate handler.
    pub fn process_line(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }

        match serde_json::from_str::<SparkEvent>(line) {
            Ok(event) => self.dispatch(event),
            Err(_) => {
                self.errors += 1;
            }
        }
    }

    /// Convert accumulated state into the final parsed result.
    pub fn finish(self) -> ParsedEventLog {
        let app_id = self.app_id.unwrap_or_else(|| "unknown".to_string());

        let mut jobs: Vec<SparkJob> = self.jobs.into_values().map(|acc| acc.into_job()).collect();
        jobs.sort_by_key(|j| j.job_id);

        // Link jobs to SQL executions
        let job_to_sql: HashMap<i64, i64> = self
            .sql_executions
            .values()
            .flat_map(|sql| sql.job_ids.iter().map(|jid| (*jid, sql.id)))
            .collect();

        let mut stages: Vec<SparkStage> = self
            .stages
            .into_values()
            .map(|acc| acc.into_stage())
            .collect();
        stages.sort_by_key(|s| s.stage_id);

        let executors: Vec<SparkExecutor> = self
            .executors
            .into_values()
            .map(|acc| SparkExecutor {
                id: acc.id,
                total_cores: acc.total_cores,
                max_memory: 0,
                is_active: acc.is_active,
            })
            .collect();

        let active_executors: Vec<&SparkExecutor> = executors
            .iter()
            .filter(|e| e.id != "driver" && e.is_active)
            .collect();

        let cluster_resources = ClusterResources {
            total_executor_memory: active_executors.iter().map(|e| e.max_memory).sum(),
            total_executor_cores: active_executors.iter().map(|e| e.total_cores).sum(),
            num_executors: active_executors.len(),
        };

        let sql_executions: Vec<SparkSqlExecution> = self
            .sql_executions
            .into_values()
            .map(|acc| acc.into_sql(&job_to_sql))
            .collect();

        if self.errors > 0 {
            warn!("Event log parsing: skipped {} malformed lines", self.errors);
        }

        ParsedEventLog {
            app_id,
            jobs,
            stages,
            tasks: self.tasks,
            executors,
            sql_executions,
            cluster_resources,
        }
    }

    fn dispatch(&mut self, event: SparkEvent) {
        match event {
            SparkEvent::ApplicationStart { app_id, .. } => {
                if let Some(id) = app_id {
                    self.app_id = Some(id);
                }
            }
            SparkEvent::ApplicationEnd { .. } => {}
            SparkEvent::JobStart {
                job_id,
                submission_time,
                stage_ids,
                stage_infos,
                properties,
            } => {
                self.handle_job_start(
                    job_id,
                    submission_time,
                    stage_ids,
                    &stage_infos,
                    &properties,
                );
            }
            SparkEvent::JobEnd {
                job_id,
                completion_time,
                job_result,
            } => {
                self.handle_job_end(job_id, completion_time, job_result.as_ref());
            }
            SparkEvent::StageSubmitted { stage_info } => {
                self.handle_stage_submitted(&stage_info);
            }
            SparkEvent::StageCompleted { stage_info } => {
                self.handle_stage_completed(&stage_info);
            }
            SparkEvent::TaskEnd {
                stage_id,
                stage_attempt_id,
                task_info,
                task_metrics,
                task_end_reason,
            } => {
                self.handle_task_end(
                    stage_id,
                    stage_attempt_id,
                    task_info.as_ref(),
                    task_metrics.as_ref(),
                    task_end_reason.as_ref(),
                );
            }
            SparkEvent::ExecutorAdded {
                executor_id,
                executor_info,
            } => {
                self.handle_executor_added(&executor_id, executor_info.as_ref());
            }
            SparkEvent::ExecutorRemoved { executor_id } => {
                if let Some(exec) = self.executors.get_mut(&executor_id) {
                    exec.is_active = false;
                }
            }
            SparkEvent::SqlExecutionStart {
                execution_id,
                description,
                physical_plan,
                time,
            } => {
                self.handle_sql_start(execution_id, description, physical_plan, time);
            }
            SparkEvent::SqlExecutionEnd { execution_id, time } => {
                if let Some(sql) = self.sql_executions.get_mut(&execution_id) {
                    sql.end_time = time;
                }
            }
            SparkEvent::Unknown => {}
        }
    }

    fn handle_job_start(
        &mut self,
        job_id: i64,
        submission_time: Option<i64>,
        stage_ids: Vec<i64>,
        stage_infos: &[EventStageInfo],
        properties: &HashMap<String, String>,
    ) {
        // Extract job name from callSite or description property
        let name = properties
            .get("callSite.short")
            .or_else(|| properties.get("spark.job.description"))
            .cloned()
            .unwrap_or_else(|| format!("Job {}", job_id));

        // Register stages from stage_infos
        for info in stage_infos {
            let key = (info.stage_id, info.stage_attempt_id);
            self.stages.entry(key).or_insert_with(|| StageAccumulator {
                stage_id: info.stage_id,
                attempt_id: info.stage_attempt_id,
                name: info.stage_name.clone(),
                num_tasks: info.num_tasks,
                submission_time: None,
                completion_time: None,
                status: StageStatus::Pending,
                executor_run_time: 0,
                executor_cpu_time: 0,
                input_bytes: 0,
                input_records: 0,
                output_bytes: 0,
                output_records: 0,
                shuffle_read_bytes: 0,
                shuffle_read_records: 0,
                shuffle_write_bytes: 0,
                shuffle_write_records: 0,
                memory_bytes_spilled: 0,
                disk_bytes_spilled: 0,
                peak_execution_memory: 0,
                completed_tasks: 0,
                failed_tasks: 0,
                killed_tasks: 0,
            });
        }

        // Link SQL execution to this job via properties
        if let Some(sql_id_str) = properties.get("spark.sql.execution.id")
            && let Ok(sql_id) = sql_id_str.parse::<i64>()
            && let Some(sql) = self.sql_executions.get_mut(&sql_id)
        {
            sql.job_ids.push(job_id);
        }

        let num_tasks: i64 = stage_infos.iter().map(|s| s.num_tasks).sum();

        self.jobs.insert(
            job_id,
            JobAccumulator {
                job_id,
                name,
                submission_time,
                completion_time: None,
                stage_ids,
                status: JobStatus::Running,
                num_tasks,
            },
        );
    }

    fn handle_job_end(
        &mut self,
        job_id: i64,
        completion_time: Option<i64>,
        job_result: Option<&EventJobResult>,
    ) {
        if let Some(job) = self.jobs.get_mut(&job_id) {
            job.completion_time = completion_time;
            job.status = match job_result {
                Some(r) if r.result == "JobSucceeded" => JobStatus::Succeeded,
                Some(_) => JobStatus::Failed,
                None => JobStatus::Unknown,
            };
        }
    }

    fn handle_stage_submitted(&mut self, info: &EventStageInfo) {
        let key = (info.stage_id, info.stage_attempt_id);
        let stage = self.stages.entry(key).or_insert_with(|| StageAccumulator {
            stage_id: info.stage_id,
            attempt_id: info.stage_attempt_id,
            name: info.stage_name.clone(),
            num_tasks: info.num_tasks,
            submission_time: info.submission_time,
            completion_time: None,
            status: StageStatus::Active,
            executor_run_time: 0,
            executor_cpu_time: 0,
            input_bytes: 0,
            input_records: 0,
            output_bytes: 0,
            output_records: 0,
            shuffle_read_bytes: 0,
            shuffle_read_records: 0,
            shuffle_write_bytes: 0,
            shuffle_write_records: 0,
            memory_bytes_spilled: 0,
            disk_bytes_spilled: 0,
            peak_execution_memory: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            killed_tasks: 0,
        });
        stage.submission_time = info.submission_time;
        stage.status = StageStatus::Active;
    }

    fn handle_stage_completed(&mut self, info: &EventStageInfo) {
        let key = (info.stage_id, info.stage_attempt_id);
        if let Some(stage) = self.stages.get_mut(&key) {
            stage.completion_time = info.completion_time;
            stage.status = if stage.failed_tasks > 0 {
                StageStatus::Failed
            } else {
                StageStatus::Complete
            };
        }
    }

    fn handle_task_end(
        &mut self,
        stage_id: i64,
        stage_attempt_id: i64,
        task_info: Option<&EventTaskInfo>,
        task_metrics: Option<&EventTaskMetrics>,
        task_end_reason: Option<&EventTaskEndReason>,
    ) {
        let Some(info) = task_info else { return };

        let failed = task_end_reason
            .map(|r| r.reason != "Success")
            .unwrap_or(info.failed);

        let status = if failed {
            "FAILED".to_string()
        } else {
            "SUCCESS".to_string()
        };

        let duration = match (info.launch_time, info.finish_time) {
            (Some(start), Some(end)) => Some(end - start),
            _ => None,
        };

        let (executor_run_time, executor_cpu_time) = task_metrics
            .map(|m| (m.executor_run_time, m.executor_cpu_time))
            .unwrap_or((0, 0));

        let (input_bytes, input_records) = task_metrics
            .and_then(|m| m.input_metrics.as_ref())
            .map(|im| (im.bytes_read, im.records_read))
            .unwrap_or((0, 0));

        let (output_bytes, output_records) = task_metrics
            .and_then(|m| m.output_metrics.as_ref())
            .map(|om| (om.bytes_written, om.records_written))
            .unwrap_or((0, 0));

        let (shuffle_read_bytes, shuffle_read_records) = task_metrics
            .and_then(|m| m.shuffle_read_metrics.as_ref())
            .map(|sr| (sr.remote_bytes_read + sr.local_bytes_read, sr.records_read))
            .unwrap_or((0, 0));

        let (shuffle_write_bytes, shuffle_write_records) = task_metrics
            .and_then(|m| m.shuffle_write_metrics.as_ref())
            .map(|sw| (sw.bytes_written, sw.records_written))
            .unwrap_or((0, 0));

        let memory_bytes_spilled = task_metrics.map(|m| m.memory_bytes_spilled).unwrap_or(0);
        let disk_bytes_spilled = task_metrics.map(|m| m.disk_bytes_spilled).unwrap_or(0);
        let peak_execution_memory = task_metrics.map(|m| m.peak_execution_memory).unwrap_or(0);

        let task = SparkTask {
            task_id: info.task_id,
            index: info.index,
            attempt: info.attempt,
            stage_id,
            stage_attempt_id,
            executor_id: info.executor_id.clone(),
            host: info.host.clone(),
            status,
            duration,
            executor_run_time,
            executor_cpu_time,
            input_bytes,
            input_records,
            output_bytes,
            output_records,
            shuffle_read_bytes,
            shuffle_read_records,
            shuffle_write_bytes,
            shuffle_write_records,
            memory_bytes_spilled,
            disk_bytes_spilled,
            peak_execution_memory,
        };

        // Accumulate metrics into stage
        let key = (stage_id, stage_attempt_id);
        if let Some(stage) = self.stages.get_mut(&key) {
            stage.executor_run_time += executor_run_time;
            stage.executor_cpu_time += executor_cpu_time;
            stage.input_bytes += input_bytes;
            stage.input_records += input_records;
            stage.output_bytes += output_bytes;
            stage.output_records += output_records;
            stage.shuffle_read_bytes += shuffle_read_bytes;
            stage.shuffle_read_records += shuffle_read_records;
            stage.shuffle_write_bytes += shuffle_write_bytes;
            stage.shuffle_write_records += shuffle_write_records;
            stage.memory_bytes_spilled += memory_bytes_spilled;
            stage.disk_bytes_spilled += disk_bytes_spilled;
            if peak_execution_memory > stage.peak_execution_memory {
                stage.peak_execution_memory = peak_execution_memory;
            }
            if failed {
                stage.failed_tasks += 1;
            } else {
                stage.completed_tasks += 1;
            }
        }

        self.tasks.entry(stage_id).or_default().push(task);
    }

    fn handle_executor_added(&mut self, executor_id: &str, info: Option<&EventExecutorInfo>) {
        let total_cores = info.map(|i| i.total_cores).unwrap_or(0);
        self.executors.insert(
            executor_id.to_string(),
            ExecutorAccumulator {
                id: executor_id.to_string(),
                total_cores,
                is_active: true,
            },
        );
    }

    fn handle_sql_start(
        &mut self,
        execution_id: i64,
        description: Option<String>,
        physical_plan: Option<String>,
        time: Option<i64>,
    ) {
        self.sql_executions.insert(
            execution_id,
            SqlAccumulator {
                id: execution_id,
                description: description.unwrap_or_default(),
                plan_description: physical_plan.unwrap_or_default(),
                start_time: time,
                end_time: None,
                job_ids: Vec::new(),
            },
        );
    }
}

// -- Accumulator to final type conversions --

impl JobAccumulator {
    fn into_job(self) -> SparkJob {
        SparkJob {
            job_id: self.job_id,
            name: self.name,
            status: self.status,
            submission_time: self.submission_time.map(epoch_to_iso),
            completion_time: self.completion_time.map(epoch_to_iso),
            stage_ids: self.stage_ids,
            num_tasks: self.num_tasks,
            num_active_tasks: 0,
            num_completed_tasks: if self.status == JobStatus::Succeeded {
                self.num_tasks
            } else {
                0
            },
            num_skipped_tasks: 0,
            num_failed_tasks: if self.status == JobStatus::Failed {
                1
            } else {
                0
            },
            num_killed_tasks: 0,
            num_completed_indices: 0,
            num_active_stages: 0,
            num_completed_stages: 0,
            num_skipped_stages: 0,
            num_failed_stages: 0,
        }
    }
}

impl StageAccumulator {
    fn into_stage(self) -> SparkStage {
        SparkStage {
            status: self.status,
            stage_id: self.stage_id,
            attempt_id: self.attempt_id,
            num_tasks: self.num_tasks,
            num_active_tasks: 0,
            num_complete_tasks: self.completed_tasks,
            num_failed_tasks: self.failed_tasks,
            num_killed_tasks: self.killed_tasks,
            executor_run_time: self.executor_run_time,
            executor_cpu_time: self.executor_cpu_time,
            input_bytes: self.input_bytes,
            input_records: self.input_records,
            output_bytes: self.output_bytes,
            output_records: self.output_records,
            shuffle_read_bytes: self.shuffle_read_bytes,
            shuffle_read_records: self.shuffle_read_records,
            shuffle_write_bytes: self.shuffle_write_bytes,
            shuffle_write_records: self.shuffle_write_records,
            memory_bytes_spilled: self.memory_bytes_spilled,
            disk_bytes_spilled: self.disk_bytes_spilled,
            peak_execution_memory: self.peak_execution_memory,
            name: self.name,
            submission_time: self.submission_time.map(epoch_to_iso),
            completion_time: self.completion_time.map(epoch_to_iso),
            first_task_launched_time: self.submission_time.map(epoch_to_iso),
        }
    }
}

impl SqlAccumulator {
    fn into_sql(self, job_to_sql: &HashMap<i64, i64>) -> SparkSqlExecution {
        let duration = match (self.start_time, self.end_time) {
            (Some(s), Some(e)) => e - s,
            _ => 0,
        };

        let success_job_ids: Vec<i64> = self
            .job_ids
            .iter()
            .filter(|jid| job_to_sql.get(jid).is_some_and(|sid| *sid == self.id))
            .copied()
            .collect();

        SparkSqlExecution {
            id: self.id,
            status: if self.end_time.is_some() {
                "COMPLETED".to_string()
            } else {
                "RUNNING".to_string()
            },
            description: self.description,
            plan_description: self.plan_description,
            submission_time: self
                .start_time
                .map(epoch_to_iso)
                .unwrap_or_else(|| "unknown".to_string()),
            duration,
            running_job_ids: Vec::new(),
            success_job_ids,
            failed_job_ids: Vec::new(),
        }
    }
}

/// Convert epoch milliseconds to ISO 8601 string.
fn epoch_to_iso(epoch_ms: i64) -> String {
    use chrono::{TimeZone, Utc};
    let secs = epoch_ms / 1000;
    let nanos = ((epoch_ms % 1000) * 1_000_000) as u32;
    match Utc.timestamp_opt(secs, nanos) {
        chrono::LocalResult::Single(dt) => dt.to_rfc3339(),
        _ => format!("epoch:{}", epoch_ms),
    }
}
