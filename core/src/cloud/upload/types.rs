use super::*;
use crate::library::AudioMetadata;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub trait UploadClock: Send + Sync {
    fn now_ms(&self) -> Result<u64>;
}
pub(super) struct SystemClock;
impl UploadClock for SystemClock {
    fn now_ms(&self) -> Result<u64> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::State("clock precedes upload epoch"))?
            .as_millis()
            .try_into()
            .map_err(|_| Error::State("upload clock overflow"))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudPhase {
    Queued,
    Checked,
    AllocationPending,
    Allocated,
    AudioUploading,
    AudioUploaded,
    AttachmentsUploading,
    MetadataPending,
    Encoding,
    ReadyToPublish,
    PublicationPending,
    Published,
    Complete,
    Failed,
    Cancelled,
}
impl CloudPhase {
    pub(super) fn pending(self) -> bool {
        matches!(
            self,
            Self::AllocationPending | Self::MetadataPending | Self::PublicationPending
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentPhase {
    Queued,
    AllocationPending,
    Allocated,
    Uploading,
    Uploaded,
    Omitted,
    Rejected,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadControl {
    Pause,
    Cancel,
}
#[derive(Clone, Debug, Default)]
pub struct AttachmentInputs {
    pub cover: Option<PathBuf>,
    pub lyric: Option<PathBuf>,
    pub cue: Option<PathBuf>,
}
#[derive(Clone, Debug, Default)]
pub struct CloudRunOptions {
    /// Budget per active audio/attachment child, just as the recovered adapter.
    pub chunk_budget: Option<usize>,
    pub retry_rejected: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UploadIssue {
    pub kind: String,
    pub code: Option<i64>,
}
impl UploadIssue {
    pub(super) fn new(kind: &str) -> Self {
        Self {
            kind: kind.into(),
            code: None,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct AttachmentView {
    pub kind: AttachmentKind,
    pub source: PathBuf,
    pub phase: AttachmentPhase,
    pub bytes: u64,
    pub total: u64,
    pub transfer: Option<NosStatus>,
    #[serde(skip)]
    pub(crate) unresolved_nos_boundary: bool,
    pub error: Option<UploadIssue>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "transport", content = "state", rename_all = "snake_case")]
pub enum CloudTransfer {
    Nos(NosView),
    Oss(OssView),
}
impl CloudTransfer {
    pub(super) fn bytes(&self) -> u64 {
        match self {
            Self::Nos(view) => view.bytes,
            Self::Oss(view) => view.bytes,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct CloudView {
    pub id: String,
    pub source: PathBuf,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub phase: CloudPhase,
    pub channel: Option<u64>,
    pub bytes: u64,
    pub total: u64,
    pub transfer: Option<CloudTransfer>,
    pub song_id: Option<String>,
    pub next_check_ms: u64,
    pub requested: Option<UploadControl>,
    pub error: Option<UploadIssue>,
    pub attachments: Vec<AttachmentView>,
    pub(crate) omittable_text: Option<AttachmentKind>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Decision {
    pub need_upload: bool,
    pub ticket: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Attachment {
    pub kind: AttachmentKind,
    pub source: Source,
    pub mime: String,
    pub phase: AttachmentPhase,
    pub allocation: Option<Value>,
    pub error: Option<UploadIssue>,
}
pub(super) const TEXT_BUCKET_ERROR: &str = "image_only_bucket_for_text";

impl Attachment {
    pub(super) fn incompatible_text(&self) -> bool {
        matches!(self.kind, AttachmentKind::Lyric | AttachmentKind::Cue)
            && self
                .error
                .as_ref()
                .is_some_and(|e| e.kind == TEXT_BUCKET_ERROR)
            && self
                .allocation
                .as_ref()
                .and_then(|v| NosAllocation::parse(v).ok())
                .is_some_and(|a| a.bucket() == "yyimgs")
    }
}
impl CloudView {
    pub(crate) fn unresolved_nos_boundary(&self) -> bool {
        matches!(&self.transfer, Some(CloudTransfer::Nos(v)) if v.unresolved_boundary)
            || self.attachments.iter().any(|a| a.unresolved_nos_boundary)
    }
    pub(crate) fn blocked_text(&self) -> bool {
        self.attachments.iter().any(|a| {
            a.phase == AttachmentPhase::Allocated
                && a.error
                    .as_ref()
                    .is_some_and(|e| e.kind == TEXT_BUCKET_ERROR)
        })
    }
}
// Native persistence only: allocation/decision/response fields contain private
// credentials and upload tickets. Never derive Debug or expose this over IPC.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Task {
    pub id: String,
    pub source: Source,
    pub info: AudioMetadata,
    pub phase: CloudPhase,
    pub decision: Option<Decision>,
    pub allocation: Option<Value>,
    pub song_id: Option<String>,
    pub attachments: Vec<Attachment>,
    pub next_check_ms: u64,
    pub error: Option<UploadIssue>,
    pub retry_phase: Option<CloudPhase>,
    pub metadata: Option<Value>,
    pub publication: Option<Value>,
    pub account_item: Option<Value>,
}
pub(super) fn valid_id(id: &str) -> Result<()> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::InvalidInput("invalid cloud upload task ID"));
    }
    Ok(())
}
pub(super) fn file_name(source: &Source) -> Result<&str> {
    let name = source
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or(Error::InvalidInput("cloud file name must be Unicode"))?;
    super::super::api::validate_file_name(name)?;
    Ok(name)
}
impl Task {
    pub fn validate(&self) -> Result<()> {
        valid_id(&self.id)?;
        self.source.validate()?;
        file_name(&self.source)?;
        if !self.info.duration.is_finite() || self.info.duration < 0.0 || self.attachments.len() > 3
        {
            return Err(Error::State("invalid cloud upload metadata"));
        }
        let mut kinds = Vec::new();
        for attachment in &self.attachments {
            attachment.source.validate()?;
            file_name(&attachment.source)?;
            if kinds.contains(&attachment.kind) || attachment.source.bytes > 4 * 1024 * 1024 {
                return Err(Error::State("invalid persisted cloud attachments"));
            }
            if attachment.phase == AttachmentPhase::Omitted && !attachment.incompatible_text() {
                return Err(Error::State("invalid omitted cloud attachment"));
            }
            kinds.push(attachment.kind);
        }
        if let Some(decision) = &self.decision
            && (decision.ticket.is_empty()
                || decision.ticket.chars().count() > 4096
                || decision.ticket.contains(['\r', '\n', '\0']))
        {
            return Err(Error::State("invalid persisted cloud ticket"));
        }
        if let Some(id) = &self.song_id {
            crate::protocol::api::identifier(&json!(id))?;
        }
        Ok(())
    }
    pub fn uncertain(&self) -> bool {
        self.phase.pending()
            || self
                .attachments
                .iter()
                .any(|a| a.phase == AttachmentPhase::AllocationPending)
    }
    pub(super) fn omittable_text(&self) -> Option<AttachmentKind> {
        if self.phase != CloudPhase::AttachmentsUploading || self.uncertain() {
            return None;
        }
        let mut candidate = None;
        for attachment in &self.attachments {
            match attachment.phase {
                AttachmentPhase::Queued | AttachmentPhase::Uploaded | AttachmentPhase::Omitted => {}
                AttachmentPhase::Allocated
                    if candidate.is_none() && attachment.incompatible_text() =>
                {
                    candidate = Some(attachment.kind);
                }
                _ => return None,
            }
        }
        candidate
    }
    pub fn child_id(&self, kind: Option<AttachmentKind>) -> String {
        format!(
            "{}-{}",
            self.id,
            match kind {
                None => "audio",
                Some(AttachmentKind::Cover) => "cover",
                Some(AttachmentKind::Lyric) => "lyric",
                Some(AttachmentKind::Cue) => "cue",
            }
        )
    }
    pub fn child_ids(&self) -> Vec<String> {
        std::iter::once(self.child_id(None))
            .chain(self.attachments.iter().map(|a| self.child_id(Some(a.kind))))
            .collect()
    }
}
