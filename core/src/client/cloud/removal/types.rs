use super::*;
use crate::{cloud::library::CloudDeletionKind, download::completed::DeleteSource};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudRemovalFileState {
    Pending,
    Removing,
    Removed,
    Missing,
    Retained,
    Failed,
    Skipped,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudRemovalPhase {
    Prepared,
    Local,
    Remote,
    Result,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudRemovalFileView {
    pub cloud_id: String,
    pub title: String,
    pub state: CloudRemovalFileState,
    pub issue: Option<&'static str>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudRemovalView {
    pub epoch: String,
    pub id: String,
    pub revision: String,
    pub ids: Vec<String>,
    pub titles: Vec<String>,
    pub kinds: Vec<CloudDeletionKind>,
    pub phase: CloudRemovalPhase,
    pub running: bool,
    pub delete_local: Option<bool>,
    pub local_count: usize,
    pub cue_count: usize,
    pub local_issue: Option<&'static str>,
    pub files: Vec<CloudRemovalFileView>,
    pub deletion: Option<CloudDeleteView>,
    pub legacy_only: bool,
    pub can_continue: bool,
    pub error: Option<&'static str>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Origin {
    Upload { id: String },
    Download { id: String, operation_id: String },
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Issue {
    Changed,
    Cue,
    Unavailable,
    Journal,
    Preflight,
    RemoteChanged,
    RemoteUnavailable,
}
impl Issue {
    pub fn message(self) -> &'static str {
        match self {
            Self::Changed => "文件或关联已变化，已保留",
            Self::Cue => "CUE音源已保留",
            Self::Unavailable => "本地文件暂无法处理，已保留",
            Self::Journal => "文件操作尚未完成，可在下载管理中核对",
            Self::Preflight => "删除范围已变化，请返回列表重新确认",
            Self::RemoteChanged => "本地处理已完成，云盘列表已变化；云盘删除未提交，请返回列表核对",
            Self::RemoteUnavailable => "操作尚未完成，请核对记录后继续",
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct FileRecord {
    pub cloud_id: String,
    pub title: String,
    pub origin: Origin,
    pub source: DeleteSource,
    pub state: CloudRemovalFileState,
    pub issue: Option<Issue>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Plan {
    pub id: String,
    pub created_ms: u64,
    pub revision: String,
    pub ids: Vec<String>,
    pub titles: Vec<String>,
    pub kinds: Vec<CloudDeletionKind>,
    pub files: Vec<FileRecord>,
    pub cue_count: usize,
    pub local_issue: bool,
    pub choice: Option<bool>,
    pub local_done: bool,
    pub blocked: bool,
    pub issue: Option<Issue>,
}
impl Plan {
    pub fn validate(&self) -> Result<()> {
        valid_id(&self.id)?;
        checked_ids(&self.ids)?;
        if self.revision.len() != 64
            || self.ids.len() != self.titles.len()
            || self.ids.len() != self.kinds.len()
            || self.files.len() > self.ids.len()
            || self.cue_count > self.ids.len()
        {
            return Err(Error::State("cloud removal plan is inconsistent"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for f in &self.files {
            if !self.ids.contains(&f.cloud_id)
                || !seen.insert(&f.cloud_id)
                || !f.source.path.is_absolute()
            {
                return Err(Error::State("cloud removal file is inconsistent"));
            }
            f.source.facts.integrity.validate()?;
            match &f.origin {
                Origin::Upload { id } => valid_id(id)?,
                Origin::Download { id, operation_id } => {
                    valid_id(id)?;
                    valid_id(operation_id)?;
                }
            }
        }
        Ok(())
    }
}
