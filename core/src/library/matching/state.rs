use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchPhase {
    Queued,
    Running,
    Paused,
    Failed,
    Complete,
    Cancelled,
}
impl MatchPhase {
    pub fn runnable(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
    pub fn terminal(self) -> bool {
        matches!(self, Self::Complete | Self::Cancelled)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchJob {
    pub id: String,
    pub phase: MatchPhase,
    pub total: usize,
    pub processed: usize,
    pub matched: usize,
    pub unmatched: usize,
    pub skipped: usize,
    /// Changes on controls and batch commits, rejecting stale async results.
    pub revision: u64,
    /// Stable classification; never persists API body, file path or credentials.
    pub error: Option<String>,
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchAction {
    Pause,
    Resume,
    Cancel,
}

pub(super) const JOBS: &str = "local_match_jobs_v1";
pub(super) const CANDIDATES: &str = "local_match_candidates_v1";
pub(super) const CURRENT: &str = "current";
pub(super) fn key(job: &MatchJob, index: usize) -> String {
    format!("{}:{index:08}", job.id)
}
