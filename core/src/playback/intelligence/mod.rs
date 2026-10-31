//! M/308 aiPlay/switchPlayingMode and M/1384/M/176 queue transitions. HTTP,
//! rights/blacklist filtering and media ownership belong to the caller. These
//! transitions are durable native queue operations, not an alternate player.
mod insertion;
mod plan;
mod recovery;
mod restore;
use crate::{
    Error, Result,
    catalog::intelligence::{IntelligenceRecommendation, IntelligenceRequest, MAX_LIKED_TRACKS},
    playback::{PlaybackMode, QueueEntry, QueueSnapshot},
    protocol::api::identifier,
};
pub use recovery::IntelligenceRecovery;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

pub(crate) const KIND: &str = "playback-intelligence-v1";
pub(crate) const KEY: &str = "session";
pub const MAX_QUEUE_TRACKS: usize = 10_000; // Official M/7.g.
pub(crate) const TRANSIENT: &str = "intelligence_transient";
pub(crate) const RETAINED: &str = "intelligence_retained";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntelligencePhase {
    Seeded,
    Active,
    Fallback,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntelligenceContext {
    pub id: String,
    pub phase: IntelligencePhase,
}
impl IntelligenceContext {
    pub(crate) fn validate(&self) -> Result<()> {
        Uuid::parse_str(&self.id).map_err(|_| Error::State("invalid intelligence session id"))?;
        Ok(())
    }
    pub fn active(&self) -> bool {
        self.phase != IntelligencePhase::Fallback
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceFailure {
    RequestFailed,
    InvalidRecommendations,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IntelligenceSession {
    schema: u8,
    pub revision: String,
    pub context: IntelligenceContext,
    pub request: IntelligenceRequest,
    pub seed_entry_id: String,
    pub last_mode: PlaybackMode,
    pub backup: QueueSnapshot,
    pub fallback: QueueSnapshot,
    pub recommendations: BTreeMap<String, IntelligenceRecommendation>,
    pub failure: Option<IntelligenceFailure>,
}
impl IntelligenceSession {
    pub(crate) fn validate(&self, queue: &QueueSnapshot) -> Result<()> {
        if self.schema != 1 || queue.intelligence.as_ref() != Some(&self.context) {
            return Err(Error::State("intelligence queue/session mismatch"));
        }
        self.context.validate()?;
        Uuid::parse_str(&self.revision)
            .map_err(|_| Error::State("invalid intelligence state revision"))?;
        if self.seed_entry_id.is_empty()
            || (self.context.phase == IntelligencePhase::Fallback) != self.failure.is_some()
            || (self.context.active() && queue.mode != PlaybackMode::OrderLoop)
        {
            return Err(Error::State("invalid intelligence state"));
        }
        self.request.payload()?;
        ordinary(&self.backup)?;
        liked(&self.fallback)?;
        if self.recommendations.len() > MAX_LIKED_TRACKS {
            return Err(Error::State("too many intelligence recommendation records"));
        }
        for (key, value) in &self.recommendations {
            if identifier(&json!(key))? != identifier(&json!(value.id))? {
                return Err(Error::State(
                    "intelligence recommendation identity mismatch",
                ));
            }
        }
        Ok(())
    }
}

/// The input fallback is the already filtered complete liked queue. It carries
/// no coordinator context; the original request count remains unfiltered.
pub struct IntelligenceBegin {
    pub request: IntelligenceRequest,
    pub seed_resource: Value,
    pub seed_source: Value,
    pub liked: QueueSnapshot,
    pub preserve_current_queue: bool,
}
pub enum IntelligenceAction {
    Begin(Box<IntelligenceBegin>),
    Complete {
        session_id: String,
        /// Already authorized/playable resources, before M/59 first-ID dedup.
        resources: Vec<Value>,
        recommendations: BTreeMap<String, IntelligenceRecommendation>,
    },
    Fail {
        session_id: String,
        failure: IntelligenceFailure,
    },
    Leave {
        session_id: String,
        mode: PlaybackMode,
        /// M/308 PH-PC-favouriteSongSwitch=t1. Default clients must not claim
        /// that an unobserved experiment assignment was received from a server.
        retain_missing_current: bool,
    },
}
pub(crate) struct Plan {
    pub queue: QueueSnapshot,
    pub session: Option<IntelligenceSession>,
    pub preserve_media: bool,
    pub start_seed: bool,
    pub randomize: bool,
}
fn ordinary(queue: &QueueSnapshot) -> Result<()> {
    if queue.intelligence.is_some()
        || queue.fm_source.is_some()
        || queue.items.iter().any(|e| e.source["kind"] == "fm")
        || queue.items.len() > MAX_LIKED_TRACKS
    {
        return Err(Error::InvalidInput("nested intelligence/FM backup"));
    }
    queue.validate()
}
fn liked(queue: &QueueSnapshot) -> Result<()> {
    ordinary(queue)?;
    if queue.items.is_empty() {
        return Err(Error::InvalidInput(
            "intelligence requires playable liked tracks",
        ));
    }
    for entry in &queue.items {
        resource_id(&entry.resource)?;
    }
    Ok(())
}
pub(crate) fn resource_id(resource: &Value) -> Result<String> {
    if resource["missing"] == true {
        return Err(Error::InvalidInput("unresolved intelligence track"));
    }
    identifier(&resource["id"])
}
fn same_media(a: Option<&QueueEntry>, b: Option<&QueueEntry>) -> bool {
    a.zip(b)
        .is_some_and(|(a, b)| a.entry_id == b.entry_id && a.resource == b.resource)
}
pub(crate) use plan::transition;
