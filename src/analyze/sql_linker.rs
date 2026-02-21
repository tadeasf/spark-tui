use std::collections::HashMap;

use crate::fetch::types::{SparkJob, SparkSqlExecution, SparkStage, StageStatus};

use super::types::SqlJobLink;

/// Build a mapping from job_id -> sql_id.
pub fn build_job_to_sql_map(sql_executions: &[SparkSqlExecution]) -> HashMap<i64, i64> {
    let mut map = HashMap::new();
    for sql in sql_executions {
        for &job_id in &sql.running_job_ids {
            map.insert(job_id, sql.id);
        }
        for &job_id in &sql.success_job_ids {
            map.insert(job_id, sql.id);
        }
        for &job_id in &sql.failed_job_ids {
            map.insert(job_id, sql.id);
        }
    }
    map
}

/// Build a mapping from stage_id -> job_id (first job that contains that stage).
pub fn build_stage_to_job_map(jobs: &[SparkJob]) -> HashMap<i64, i64> {
    let mut map = HashMap::new();
    for job in jobs {
        for &stage_id in &job.stage_ids {
            map.entry(stage_id).or_insert(job.job_id);
        }
    }
    map
}

/// Build SqlJobLink list from SQL executions.
pub fn link_sql_to_jobs(sql_executions: &[SparkSqlExecution]) -> Vec<SqlJobLink> {
    sql_executions
        .iter()
        .map(|sql| {
            let mut job_ids: Vec<i64> = Vec::new();
            job_ids.extend(&sql.running_job_ids);
            job_ids.extend(&sql.success_job_ids);
            job_ids.extend(&sql.failed_job_ids);
            job_ids.sort();
            job_ids.dedup();
            SqlJobLink {
                sql_id: sql.id,
                sql_description: sql.description.clone(),
                job_ids,
            }
        })
        .collect()
}

/// Find the SQL description for a given job_id.
pub fn find_sql_for_job(
    job_id: i64,
    job_to_sql: &HashMap<i64, i64>,
    sql_executions: &[SparkSqlExecution],
) -> (Option<i64>, Option<String>) {
    if let Some(&sql_id) = job_to_sql.get(&job_id) {
        let desc = sql_executions
            .iter()
            .find(|s| s.id == sql_id)
            .map(|s| s.description.clone());
        (Some(sql_id), desc)
    } else {
        (None, None)
    }
}

/// Select stages for task-level analysis using multiple heuristics.
/// Combines top-by-runtime, top-by-shuffle, and high-parallelism stages.
/// Capped at 15 to keep API calls bounded.
pub fn stages_for_task_analysis(stages: &[SparkStage]) -> Vec<(i64, i64)> {
    let completed: Vec<&SparkStage> = stages
        .iter()
        .filter(|s| s.status == StageStatus::Complete)
        .collect();

    let mut selected: HashMap<i64, i64> = HashMap::new();

    // Top 5 by executor_run_time
    let mut by_runtime = completed.clone();
    by_runtime.sort_by(|a, b| b.executor_run_time.cmp(&a.executor_run_time));
    for s in by_runtime.iter().take(5) {
        selected.entry(s.stage_id).or_insert(s.attempt_id);
    }

    // Top 5 by shuffle_write_bytes
    let mut by_shuffle = completed.clone();
    by_shuffle.sort_by(|a, b| b.shuffle_write_bytes.cmp(&a.shuffle_write_bytes));
    for s in by_shuffle.iter().take(5).filter(|s| s.shuffle_write_bytes > 0) {
        selected.entry(s.stage_id).or_insert(s.attempt_id);
    }

    // High-parallelism stages (>100 tasks)
    for s in completed.iter().filter(|s| s.num_tasks > 100) {
        selected.entry(s.stage_id).or_insert(s.attempt_id);
    }

    // Cap at 15
    selected.into_iter().take(15).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::types::*;

    fn make_sql(id: i64, success_jobs: Vec<i64>) -> SparkSqlExecution {
        SparkSqlExecution {
            id,
            status: "COMPLETED".to_string(),
            description: format!("SQL query {}", id),
            plan_description: String::new(),
            submission_time: String::new(),
            duration: 1000,
            running_job_ids: vec![],
            success_job_ids: success_jobs,
            failed_job_ids: vec![],
        }
    }

    #[test]
    fn test_build_job_to_sql_map() {
        let sqls = vec![make_sql(1, vec![10, 11]), make_sql(2, vec![12])];
        let map = build_job_to_sql_map(&sqls);
        assert_eq!(map.get(&10), Some(&1));
        assert_eq!(map.get(&11), Some(&1));
        assert_eq!(map.get(&12), Some(&2));
        assert_eq!(map.get(&99), None);
    }

    #[test]
    fn test_link_sql_to_jobs() {
        let sqls = vec![make_sql(1, vec![10, 11])];
        let links = link_sql_to_jobs(&sqls);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].sql_id, 1);
        assert_eq!(links[0].job_ids, vec![10, 11]);
    }
}
