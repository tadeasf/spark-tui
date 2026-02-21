use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::analyze::skew::detect_skew;
use crate::analyze::sql_linker::{
    build_job_to_sql_map, build_stage_to_job_map, find_sql_for_job, stages_for_task_analysis,
};
use crate::analyze::suspects::{
    aggregate_suspects, detect_cpu_efficiency, detect_memory_pressure, detect_record_explosion,
    detect_slow_stages, detect_spill, detect_task_failures,
};
use crate::analyze::types::{HealthSummary, RankedJob, Severity};
use crate::fetch::client::SparkHttpClient;
use crate::fetch::types::ClusterResources;
use crate::tui::{Action, DataPayload};
use crate::util::time::duration_between;

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

async fn poll_once(
    client: &SparkHttpClient,
    app_id: &str,
) -> Result<DataPayload, crate::fetch::client::FetchError> {
    // Fetch jobs, stages, SQL, executors concurrently
    let (jobs_res, stages_res, sql_res, executors_res) = tokio::join!(
        client.get_jobs(app_id),
        client.get_stages(app_id),
        client.get_sql_executions(app_id),
        client.get_executors(app_id),
    );

    let jobs = jobs_res?;
    let stages = stages_res?;
    let sql_executions = sql_res?;

    let cluster_resources = match executors_res {
        Ok(executors) => {
            let active: Vec<_> = executors
                .iter()
                .filter(|e| e.id != "driver" && e.is_active)
                .collect();
            ClusterResources {
                total_executor_memory: active.iter().map(|e| e.max_memory).sum(),
                total_executor_cores: active.iter().map(|e| e.total_cores).sum(),
                num_executors: active.len(),
            }
        }
        Err(e) => {
            warn!("Failed to fetch executor data: {}", e);
            ClusterResources::default()
        }
    };

    // Build maps
    let job_to_sql = build_job_to_sql_map(&sql_executions);
    let stage_to_job = build_stage_to_job_map(&jobs);

    // Build sql_descriptions: sql_id -> description
    let sql_descriptions: HashMap<i64, String> = sql_executions
        .iter()
        .map(|sql| (sql.id, sql.description.clone()))
        .collect();

    // Build sql plan_description lookup: sql_id -> plan_description
    let sql_plans: HashMap<i64, String> = sql_executions
        .iter()
        .map(|sql| (sql.id, sql.plan_description.clone()))
        .collect();

    // Build stage_name lookup
    let stage_names: HashMap<i64, &str> = stages
        .iter()
        .map(|s| (s.stage_id, s.name.as_str()))
        .collect();

    // Build ranked jobs (sorted by duration desc)
    let mut ranked_jobs: Vec<RankedJob> = jobs
        .iter()
        .map(|j| {
            let duration_ms =
                duration_between(j.submission_time.as_deref(), j.completion_time.as_deref());
            let (sql_id, sql_desc) = find_sql_for_job(j.job_id, &job_to_sql, &sql_executions);
            let sql_plan = sql_id.and_then(|sid| sql_plans.get(&sid).cloned());
            RankedJob {
                job_id: j.job_id,
                name: j.name.clone(),
                status: format!("{:?}", j.status).to_uppercase(),
                duration_ms,
                num_tasks: j.num_tasks,
                num_failed_tasks: j.num_failed_tasks,
                sql_id,
                sql_description: sql_desc,
                stage_ids: j.stage_ids.clone(),
                submission_time: j.submission_time.clone(),
                sql_plan,
            }
        })
        .collect();

    // Sort: running jobs first (no duration), then by duration descending
    ranked_jobs.sort_by(|a, b| match (&a.duration_ms, &b.duration_ms) {
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
        (Some(a_ms), Some(b_ms)) => b_ms.cmp(a_ms),
    });

    // Stage-level analysis (no extra API calls)
    let mut all_suspects = Vec::new();
    all_suspects.extend(detect_slow_stages(
        &stages,
        &stage_to_job,
        &job_to_sql,
        &sql_descriptions,
        &sql_plans,
    ));
    all_suspects.extend(detect_spill(
        &stages,
        &stage_to_job,
        &job_to_sql,
        &sql_descriptions,
        &sql_plans,
    ));
    all_suspects.extend(detect_cpu_efficiency(
        &stages,
        &stage_to_job,
        &job_to_sql,
        &sql_descriptions,
        &sql_plans,
    ));
    all_suspects.extend(detect_record_explosion(
        &stages,
        &stage_to_job,
        &job_to_sql,
        &sql_descriptions,
        &sql_plans,
    ));
    all_suspects.extend(detect_task_failures(
        &stages,
        &stage_to_job,
        &job_to_sql,
        &sql_descriptions,
        &sql_plans,
    ));
    all_suspects.extend(detect_memory_pressure(
        &stages,
        &stage_to_job,
        &job_to_sql,
        &sql_descriptions,
        &sql_plans,
    ));

    // Fetch task lists for stages selected by heuristic analysis
    let top_stages = stages_for_task_analysis(&stages);
    let mut stage_tasks: HashMap<i64, Vec<crate::fetch::types::SparkTask>> = HashMap::new();
    for (stage_id, attempt_id) in &top_stages {
        match client.get_task_list(app_id, *stage_id, *attempt_id).await {
            Ok(tasks) => {
                let job_id = stage_to_job.get(stage_id).copied();
                let sname = stage_names.get(stage_id).copied();
                let sql_id = job_id.and_then(|jid| job_to_sql.get(&jid).copied());
                let sql_desc = sql_id.and_then(|sid| sql_descriptions.get(&sid));
                let mut skew_suspects = detect_skew(
                    &tasks,
                    *stage_id,
                    job_id,
                    sname,
                    sql_id,
                    sql_desc.map(|s| s.as_str()),
                );
                // Enrich with sql_plan_hint
                #[allow(clippy::collapsible_if)]
                if let Some(sid) = sql_id {
                    if let Some(plan) = sql_plans.get(&sid) {
                        let ops = crate::util::format::parse_plan_top_operations(plan, 3);
                        if !ops.is_empty() {
                            let hint = ops.join(" → ");
                            for suspect in &mut skew_suspects {
                                suspect.sql_plan_hint = Some(hint.clone());
                            }
                        }
                    }
                }
                all_suspects.extend(skew_suspects);
                // Persist tasks for UI stage detail view
                stage_tasks.insert(*stage_id, tasks);
            }
            Err(e) => {
                warn!(
                    "Failed to fetch tasks for stage {}/{}: {}",
                    stage_id, attempt_id, e
                );
            }
        }
    }

    let suspects = aggregate_suspects(all_suspects);

    // Compute health summary
    let summary = compute_health_summary(&ranked_jobs, &stages, &suspects);

    let now = chrono::Utc::now().format("%H:%M:%S").to_string();

    Ok(DataPayload {
        app_id: app_id.to_string(),
        jobs: ranked_jobs,
        stages,
        sql_executions,
        suspects,
        stage_tasks: std::sync::Arc::new(stage_tasks),
        summary,
        cluster_resources,
        last_updated: now,
    })
}

fn compute_health_summary(
    jobs: &[RankedJob],
    stages: &[crate::fetch::types::SparkStage],
    suspects: &[crate::analyze::types::Suspect],
) -> HealthSummary {
    let total_jobs = jobs.len();
    let running_jobs = jobs.iter().filter(|j| j.status == "RUNNING").count();
    let failed_jobs = jobs.iter().filter(|j| j.status == "FAILED").count();

    let total_input_bytes: i64 = stages.iter().map(|s| s.input_bytes).sum();
    let total_output_bytes: i64 = stages.iter().map(|s| s.output_bytes).sum();
    let total_shuffle_bytes: i64 = stages
        .iter()
        .map(|s| s.shuffle_read_bytes + s.shuffle_write_bytes)
        .sum();

    let critical_count = suspects
        .iter()
        .filter(|s| s.severity == Severity::Critical)
        .count();
    let warning_count = suspects
        .iter()
        .filter(|s| s.severity == Severity::Warning)
        .count();

    // Top 3 critical issues for the summary line
    let top_issues: Vec<String> = suspects
        .iter()
        .filter(|s| s.severity == Severity::Critical)
        .take(3)
        .map(|s| format!("{} in stage {}", s.category, s.stage_id))
        .collect();

    HealthSummary {
        total_jobs,
        running_jobs,
        failed_jobs,
        total_input_bytes,
        total_output_bytes,
        total_shuffle_bytes,
        critical_count,
        warning_count,
        top_issues,
    }
}
