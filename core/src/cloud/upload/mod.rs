//! Durable cloud publication, from U:audio_upload_task and official M/3127.
//! An accepted media object is not yet a published account cloud item.
mod attachments;
mod controls;
mod manager;
mod publication;
mod run;
mod scheduler;
mod selection;
mod transfer;
mod types;
use super::{AttachmentKind, CloudApi, nos::*, oss::*, source::Source};
use crate::{Error, Result};
pub use manager::{CloudUploads, UploadAdapters};
pub use scheduler::{UploadSchedule, UploadScheduler};
pub use selection::select_audio_sources;
use serde_json::{Value, json};
use types::{Attachment, Decision, Task};
pub use types::{
    AttachmentInputs, AttachmentPhase, AttachmentView, CloudPhase, CloudRunOptions, CloudTransfer,
    CloudView, UploadClock, UploadControl, UploadIssue,
};
