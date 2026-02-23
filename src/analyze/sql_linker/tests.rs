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
