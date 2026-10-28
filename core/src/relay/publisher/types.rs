use super::super::{
    RelayCommand, RelayConfig, RelayMachine, RelayMode, RelayResource, RelaySource, RelayUpload,
    SnapshotFrame, SnapshotItem,
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// Whitelisted native queue image. No metadata, paths, Cookies or media URLs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationFrame {
    pub queue_revision: u64,
    pub items: Vec<SnapshotItem>,
    pub selected_entry: Option<String>,
    pub current: Option<RelayResource>,
    pub source: Option<RelaySource>,
    pub mode: Option<RelayMode>,
    pub eligible_scene: bool,
}
impl PublicationFrame {
    pub fn validate(&self) -> Result<()> {
        if self.items.len() > super::super::types::MAX_RESOURCES {
            return Err(Error::InvalidInput("relay publication queue too large"));
        }
        let mut ids = std::collections::HashSet::new();
        for item in &self.items {
            item.resource.validate()?;
            if item.entry_id.is_empty() || item.entry_id.len() > 128 || !ids.insert(&item.entry_id)
            {
                return Err(Error::InvalidInput("invalid relay publication entry"));
            }
        }
        match (&self.current, &self.selected_entry) {
            (Some(current), Some(selected))
                if self
                    .items
                    .iter()
                    .any(|i| &i.entry_id == selected && &i.resource == current) =>
            {
                current.validate()?
            }
            (None, None) => (),
            _ => {
                return Err(Error::InvalidInput(
                    "relay publication current item mismatch",
                ));
            }
        }
        if let Some(source) = &self.source {
            source.validate()?;
        }
        if self.eligible_scene && self.current.is_some() && self.source.is_none() {
            return Err(Error::InvalidInput("relay publication source missing"));
        }
        Ok(())
    }
    pub fn can_publish(&self) -> bool {
        self.eligible_scene && self.current.is_some() && self.source.is_some()
    }
    pub(super) fn snapshot<'a>(&'a self, config: &'a RelayConfig) -> Result<SnapshotFrame<'a>> {
        if !self.can_publish() {
            return Err(Error::State("current queue cannot be published"));
        }
        Ok(SnapshotFrame {
            items: &self.items,
            selected_entry: self.selected_entry.as_deref(),
            current: self
                .current
                .clone()
                .ok_or(Error::State("relay publication selection missing"))?,
            config,
            mode: self.mode,
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublicationEvent {
    Source,
    Command { command: RelayCommand },
    Select,
    Mode { from_handoff: bool },
    VoiceProgress { progress_ms: u64 },
    ColdStart,
    Snapshot,
    Clear,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationPhase {
    Prepared,
    Sending,
    Confirmed,
    Rejected,
    Uncertain,
    Skipped,
    Superseded,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublicationIntent {
    pub id: String,
    pub sequence: u64,
    pub phase: PublicationPhase,
    pub upload: RelayUpload,
    pub recovery_of: Option<String>,
    pub frame_revision: u64,
    pub service_code: Option<i64>,
    pub created_at: u64,
    pub(crate) device_id: String,
}
/// Durable outcome without a queue/upload body. Compacting an old request does
/// not change its phase or make its UUID available for another operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublicationReceipt {
    pub id: String,
    pub sequence: u64,
    pub phase: PublicationPhase,
    pub recovery_of: Option<String>,
    pub frame_revision: u64,
    pub service_code: Option<i64>,
    pub created_at: u64,
    pub(crate) device_id: String,
}
impl From<PublicationIntent> for PublicationReceipt {
    fn from(row: PublicationIntent) -> Self {
        Self {
            id: row.id,
            sequence: row.sequence,
            phase: row.phase,
            recovery_of: row.recovery_of,
            frame_revision: row.frame_revision,
            service_code: row.service_code,
            created_at: row.created_at,
            device_id: row.device_id,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(super) struct PublicationState {
    pub device_id: String,
    pub sequence: u64,
    pub frame: Option<PublicationFrame>,
    pub machine: Option<RelayMachine>,
    pub blocked: Option<String>,
    pub recovery: Option<String>,
    #[serde(default)]
    pub needs_snapshot: bool,
    pub latest: Option<String>,
}
#[derive(Clone, Debug)]
pub struct PublicationStatus {
    pub pending: usize,
    pub blocked: Option<PublicationIntent>,
    pub recovering: Option<PublicationIntent>,
    pub latest: Option<PublicationIntent>,
    pub can_resynchronize: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishOutcome {
    Idle,
    Skipped,
    Confirmed(String),
    Blocked(String),
    Recovered { original: String, snapshot: String },
}
