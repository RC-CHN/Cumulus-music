use crate::{
    client::dto::{DownloadResult, PlaylistCollectSort, PlaylistView},
    download::service::DownloadOptions,
    membership::Quality,
    playlist::PlaylistPhase,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CurrentSongDownloadTarget {
    pub play_id: String,
    pub entry_id: String,
}

pub enum CurrentSongDownloadCommand {
    Prepare,
    Select {
        offer: String,
        quality: Quality,
    },
    Cancel {
        receipt: String,
    },
    Destinations {
        selection: String,
        sort: PlaylistCollectSort,
        page: u32,
    },
    ChoosePlaylist {
        selection: String,
        destination: String,
    },
    PickerStatus {
        selection: String,
        reconcile: bool,
    },
    CreatePlaylist {
        selection: String,
        name: String,
        private: bool,
    },
    CreationStatus {
        selection: String,
        reconcile: bool,
    },
    AddCreatedPlaylist {
        selection: String,
    },
    Start {
        selection: String,
        continuation: Option<String>,
        options: DownloadOptions,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CurrentSongDownloadDecision {
    Ordinary,
    Unavailable,
    PurchasedQualityRequired,
    Ready,
    PlaylistRequired,
    MembershipUnknown,
    Complete,
    ExistingTask,
    Waiting,
    Working,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentSongDownloadReply {
    pub epoch: String,
    pub play_id: String,
    pub entry_id: String,
    pub track_id: Option<String>,
    #[serde(flatten)]
    pub result: CurrentSongDownloadResult,
}

#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CurrentSongDownloadResult {
    Plan {
        decision: CurrentSongDownloadDecision,
        #[serde(skip_serializing_if = "Option::is_none")]
        offer: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        purchased_qualities: Vec<Quality>,
    },
    Selection {
        quality: Quality,
        decision: CurrentSongDownloadDecision,
        selection: Option<String>,
    },
    Cancelled,
    Destinations {
        rows: Vec<CurrentSongDownloadDestination>,
        next_page: Option<u32>,
        can_create: bool,
    },
    Picker {
        picker: CurrentSongDownloadPicker,
    },
    Creation {
        creation: CurrentSongDownloadCreation,
    },
    DirectoryRequired,
    Started {
        result: DownloadResult,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentSongDownloadDestination {
    pub receipt: String,
    pub playlist: PlaylistView,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentSongDownloadPicker {
    pub playlist_id: String,
    pub operation_id: String,
    pub phase: PlaylistPhase,
    pub add_failed: bool,
    pub continuation: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentSongDownloadCreation {
    pub create_operation_id: String,
    pub create_phase: PlaylistPhase,
    pub playlist_id: Option<String>,
    pub add: Option<CurrentSongDownloadPicker>,
    pub can_resume_add: bool,
}
