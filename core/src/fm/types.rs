use crate::{Error, Result, account::resources::MutationPhase, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FmSource {
    #[default]
    Personal,
    Familiar {
        source_id: String,
    },
    Zone {
        rule_id: String,
        #[serde(default)]
        trial_mode: Option<i32>,
    },
}
impl FmSource {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Personal => Ok(()),
            Self::Familiar { source_id } => {
                if source_id.len() > 4096 {
                    return Err(Error::InvalidInput("too many familiar FM seeds"));
                }
                // M244 passes the card's resourceId through unchanged. Live
                // cards contain a JSON array string, not one song identifier.
                if source_id.starts_with('[') {
                    let ids: Vec<Value> = serde_json::from_str(source_id)
                        .map_err(|_| Error::InvalidInput("invalid familiar FM seeds"))?;
                    if ids.is_empty() || ids.len() > 100 {
                        return Err(Error::InvalidInput("invalid familiar FM seeds"));
                    }
                    for id in ids {
                        identifier(&id)?;
                    }
                    Ok(())
                } else {
                    identifier(&json!(source_id)).map(|_| ())
                }
            }
            Self::Zone {
                rule_id,
                trial_mode,
            } => {
                identifier(&json!(rule_id))?;
                if !matches!(trial_mode, None | Some(-1 | 121)) {
                    return Err(Error::InvalidInput("invalid zone FM trial mode"));
                }
                Ok(())
            }
        }
    }
    pub fn playback_trial_mode(&self) -> i32 {
        match self {
            Self::Zone { trial_mode, .. } => trial_mode.unwrap_or(121),
            _ => 36,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FmRights {
    pub vinyl: bool,
    pub plus: bool,
}
impl From<&crate::membership::MembershipSnapshot> for FmRights {
    fn from(v: &crate::membership::MembershipSnapshot) -> Self {
        Self {
            vinyl: v.state.is_vinyl_vip,
            plus: v.state.is_plus_vip,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FmRepeat {
    #[default]
    Infinite,
    One,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FmTrack {
    pub entry_id: String,
    pub id: String,
    pub algorithm: String,
    /// Original native song data; never expose directly to a frontend.
    pub song: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FmQueue {
    pub source: FmSource,
    pub revision: u64,
    pub tracks: Vec<FmTrack>,
    pub index: usize,
    pub repeat: FmRepeat,
    pub fetched_at: u64,
    pub rights: FmRights,
    pub last_step_at: Option<u64>,
}
impl FmQueue {
    pub fn current(&self) -> Option<&FmTrack> {
        self.tracks.get(self.index)
    }
    pub fn needs_refill(&self) -> bool {
        self.tracks.is_empty() || self.index >= self.tracks.len().saturating_sub(2)
    }
    pub fn fresh_at(&self, rights: FmRights, now: u64) -> bool {
        self.source == FmSource::Personal
            && self.rights == rights
            && !self.tracks.is_empty()
            && now >= self.fetched_at
            && now - self.fetched_at <= 7_197_000
    }
    pub(crate) fn validate(&self) -> Result<()> {
        self.source.validate()?;
        if self.index > self.tracks.len() || self.tracks.len() > 50000 {
            return Err(Error::State("invalid FM cursor"));
        }
        let mut ids = BTreeSet::new();
        let mut entries = BTreeSet::new();
        for track in &self.tracks {
            if identifier(&json!(track.id))? != identifier(&track.song["id"])?
                || !ids.insert(&track.id)
                || track.entry_id.is_empty()
                || !entries.insert(&track.entry_id)
            {
                return Err(Error::State("invalid FM queue identity"));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FmBlacklist {
    pub songs: BTreeSet<String>,
    pub artists: BTreeSet<String>,
}
impl FmBlacklist {
    pub fn blocks(&self, track: &FmTrack) -> bool {
        self.songs.contains(&track.id)
            || track
                .song
                .get("artists")
                .or_else(|| track.song.get("ar"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|artist| {
                    // The official predicate compares normalized artist IDs
                    // without the positive playable-song-ID restriction.
                    let id = artist["id"]
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| artist["id"].as_u64().map(|id| id.to_string()));
                    id.is_some_and(|id| self.artists.contains(&id))
                })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FmStep {
    ManualNext,
    Ended,
    Previous,
}
/// Captured from the native player, never deserialized from IPC. A mismatching
/// selected entry reports zero time, as official M1384 does for another song.
#[derive(Clone, Debug)]
pub struct FmObservation {
    pub entry_id: String,
    pub position_seconds: f64,
}
impl FmObservation {
    pub(crate) fn seconds_for(&self, track: &FmTrack) -> Result<u64> {
        if !self.position_seconds.is_finite()
            || self.position_seconds < 0.0
            || self.position_seconds > u32::MAX as f64
        {
            return Err(Error::InvalidInput("invalid native FM clock"));
        }
        Ok(if self.entry_id == track.entry_id {
            self.position_seconds.floor() as u64
        } else {
            0
        })
    }
}
pub struct FmTransition {
    pub queue: FmQueue,
    pub skip_report: Option<String>,
    pub play: bool,
    pub exhausted: bool,
    pub refill_failed: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipPhase {
    Prepared,
    Sending,
    Confirmed,
    Rejected,
    Uncertain,
    Abandoned,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkipReport {
    pub id: String,
    pub song_id: String,
    pub algorithm: String,
    pub seconds: u64,
    pub created_at: u64,
    pub phase: SkipPhase,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FmTrashPage {
    pub page: usize,
    pub total: usize,
    pub add_time: Value,
    pub tracks: Vec<Value>,
    pub more: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FmTrashSnapshot {
    pub ids: BTreeSet<String>,
    /// Number of distinct entries actually read, not the sometimes-zero count
    /// returned by the PC service.
    pub total: usize,
    pub reported_total: usize,
    pub add_time: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FmTrashMutation {
    pub id: String,
    pub song_ids: Vec<String>,
    pub desired: bool,
    pub before: Vec<bool>,
    pub created_at: u64,
    pub phase: MutationPhase,
    pub observed: Option<Vec<bool>>,
    pub algorithm: Option<String>,
    pub seconds: u64,
    /// Optional second step selected from an authoritative reason offer. The
    /// same operation ID owns its separate blacklist journal, across restarts.
    #[serde(default)]
    pub artist_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FmArtistMutation {
    pub id: String,
    pub artist_id: String,
    pub desired: bool,
    pub before: bool,
    pub observed: Option<bool>,
    pub created_at: u64,
    pub phase: MutationPhase,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FmReason {
    pub key: String,
    pub label: String,
    pub artist_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FmReasons {
    pub entry_id: String,
    pub song_id: String,
    pub options: Vec<FmReason>,
}
