//! Same-account cross-device continuation recovered from desktop M/401 + M/15.
//! This is independent of listen-together rooms. Server eligibility is never
//! inferred from a local toggle or a cached successful submission.
mod api;
mod application;
mod machine;
pub mod publisher;
mod pull;
mod service;
mod settings;
mod types;

pub use api::{
    ALERT_PATH, CONFIG_PATH, PULL_PATH, RelayApi, SETTING_PATH, SETTING_UPDATE_PATH, SOURCE_PATH,
    STATE_PATH,
};
pub(crate) use application::{RelayPlayback, RelayPlaybackRequest, validate_id};
pub use application::{RelayPlaybackIntent, RelayPlaybackPhase};
pub use machine::{
    RelayMachine, RelayUpload, SnapshotFrame, SnapshotItem, VoiceProgressClock, snapshot_window,
};
pub use pull::{PullPlan, PullSelection};
pub use service::{PullIntent, PullPhase, RelayAvailability, RelayService, RelaySnapshot};
pub use settings::{RelaySettingIntent, RelaySettingSnapshot, RelaySettings};
pub use types::{
    RelayAlert, RelayCommand, RelayConfig, RelayMode, RelayResource, RelayResourceKind,
    RelaySource, RelaySourceKind,
};
