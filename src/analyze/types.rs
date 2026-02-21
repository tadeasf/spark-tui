use std::fmt;

/// A job ranked by duration for the Jobs tab.
#[derive(Debug, Clone)]
pub struct RankedJob {
    pub job_id: i64,
    pub name: String,
    pub status: String,
    pub duration_ms: Option<i64>,
    pub num_tasks: i64,
    pub num_failed_tasks: i64,
    pub sql_id: Option<i64>,
    pub sql_description: Option<String>,
    pub stage_ids: Vec<i64>,
    pub submission_time: Option<String>,
    pub sql_plan: Option<String>,
}

/// Severity level for suspects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Critical,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Warning => write!(f, "WARN"),
            Severity::Critical => write!(f, "CRIT"),
        }
    }
}

/// Category of a suspect finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspectCategory {
    SlowStage,
    DataSkew,
    DiskSpill,
}

impl fmt::Display for SuspectCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SuspectCategory::SlowStage => write!(f, "Slow Stage"),
            SuspectCategory::DataSkew => write!(f, "Data Skew"),
            SuspectCategory::DiskSpill => write!(f, "Disk Spill"),
        }
    }
}

/// Root-cause classification for why a stage is slow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottleneckPattern {
    LargeScan,
    WideShuffle,
    DataExplosion,
}

impl fmt::Display for BottleneckPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BottleneckPattern::LargeScan => write!(f, "Large Scan"),
            BottleneckPattern::WideShuffle => write!(f, "Wide Shuffle"),
            BottleneckPattern::DataExplosion => write!(f, "Data Explosion"),
        }
    }
}

/// A suspect finding from the analysis layer.
#[derive(Debug, Clone)]
pub struct Suspect {
    pub severity: Severity,
    pub category: SuspectCategory,
    pub stage_id: i64,
    pub job_id: Option<i64>,
    pub title: String,
    pub detail: String,
    pub stage_name: Option<String>,
    pub sql_id: Option<i64>,
    pub sql_description: Option<String>,
    pub io_summary: Option<String>,
    pub recommendation: Option<String>,
    pub bottleneck: Option<BottleneckPattern>,
    pub sql_plan_hint: Option<String>,
}

impl Suspect {
    pub fn new(
        severity: Severity,
        category: SuspectCategory,
        stage_id: i64,
        job_id: Option<i64>,
        title: String,
        detail: String,
    ) -> Self {
        Self {
            severity,
            category,
            stage_id,
            job_id,
            title,
            detail,
            stage_name: None,
            sql_id: None,
            sql_description: None,
            io_summary: None,
            recommendation: None,
            bottleneck: None,
            sql_plan_hint: None,
        }
    }
}
