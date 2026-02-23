use std::collections::HashMap;

use crate::fetch::types::SparkStage;
use crate::util::format::parse_plan_top_operations;

use super::super::types::Suspect;
use super::bottleneck::stage_io_summary;

/// Holds all lookup maps needed by suspect detectors, eliminating repetitive parameters.
pub struct SuspectContext<'a> {
    pub stage_to_job: &'a HashMap<i64, i64>,
    pub job_to_sql: &'a HashMap<i64, i64>,
    pub sql_descriptions: &'a HashMap<i64, String>,
    pub sql_plans: &'a HashMap<i64, String>,
}

impl<'a> SuspectContext<'a> {
    pub fn new(
        stage_to_job: &'a HashMap<i64, i64>,
        job_to_sql: &'a HashMap<i64, i64>,
        sql_descriptions: &'a HashMap<i64, String>,
        sql_plans: &'a HashMap<i64, String>,
    ) -> Self {
        Self {
            stage_to_job,
            job_to_sql,
            sql_descriptions,
            sql_plans,
        }
    }

    /// Look up the job_id for a stage.
    pub fn job_id(&self, stage_id: i64) -> Option<i64> {
        self.stage_to_job.get(&stage_id).copied()
    }

    /// Resolve the SQL id and description for a stage via its job.
    fn resolve_sql(&self, stage_id: i64) -> (Option<i64>, Option<String>) {
        let job_id = self.stage_to_job.get(&stage_id).copied();
        let sql_id = job_id.and_then(|jid| self.job_to_sql.get(&jid).copied());
        let sql_desc = sql_id.and_then(|sid| self.sql_descriptions.get(&sid).cloned());
        (sql_id, sql_desc)
    }

    /// Resolve the SQL plan hint for a stage.
    pub fn resolve_plan_hint_for(&self, stage_id: i64) -> Option<String> {
        let job_id = self.stage_to_job.get(&stage_id).copied()?;
        let sql_id = self.job_to_sql.get(&job_id).copied()?;
        let plan = self.sql_plans.get(&sql_id)?;
        let ops = parse_plan_top_operations(plan, 3);
        if ops.is_empty() {
            return None;
        }
        Some(ops.join(" → "))
    }

    /// Enrich a suspect with common stage metadata: stage_name, sql linkage, I/O summary, plan hint.
    pub fn enrich(&self, suspect: &mut Suspect, stage: &SparkStage) {
        let (sql_id, sql_description) = self.resolve_sql(stage.stage_id);
        suspect.stage_name = Some(stage.name.clone());
        suspect.sql_id = sql_id;
        suspect.sql_description = sql_description;
        suspect.io_summary = Some(stage_io_summary(stage));
        suspect.sql_plan_hint = self.resolve_plan_hint_for(stage.stage_id);
    }
}
