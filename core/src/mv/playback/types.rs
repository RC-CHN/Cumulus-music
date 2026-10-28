use crate::{Error, account::SessionToken, mv::MvDetail};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoStatus {
    Idle,
    Resolving,
    Loading,
    FileOperation,
    Playing,
    Paused,
    Ended,
    Error,
}
#[derive(Clone)]
pub struct VideoSnapshot {
    pub account: SessionToken,
    pub revision: u64,
    /// Accepted control intents, independent of engine clock updates.
    pub control_revision: u64,
    pub play_id: String,
    pub id: Option<String>,
    pub status: VideoStatus,
    pub detail: Option<MvDetail>,
    pub requested_resolution: u32,
    pub actual_resolution: Option<u32>,
    pub local: bool,
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub paused: bool,
    pub buffering: bool,
    pub volume: f64,
    pub speed: f64,
    pub error: Option<Arc<Error>>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VideoCommand {
    Open {
        id: String,
        resolution: u32,
        #[serde(default)]
        download_task_id: Option<String>,
    },
    Play,
    Pause {
        paused: bool,
    },
    Seek {
        seconds: f64,
    },
    Volume {
        value: f64,
    },
    Speed {
        value: f64,
    },
    Quality {
        resolution: u32,
    },
    Refresh,
    Stop,
    Snapshot,
    /// Native media focus handoff: revoke pending autoplay, pause a loaded MV.
    Suspend,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VideoFence {
    control_revision: u64,
    play_id: String,
    id: Option<String>,
}
impl VideoSnapshot {
    pub(crate) fn fence(&self) -> VideoFence {
        VideoFence {
            control_revision: self.control_revision,
            play_id: self.play_id.clone(),
            id: self.id.clone(),
        }
    }
}
