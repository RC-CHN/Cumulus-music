use crate::{
    Error, Result,
    protocol::api::{identifier, playlist_name},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistGroup {
    Created,
    Subscribed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackChange {
    Add,
    Delete,
    Reorder,
    Clear,
    /// Read-only recovery of an early audit journal. `update` is ordering,
    /// not replacement; new callers must use Reorder or explicit Clear.
    #[serde(rename = "replace")]
    LegacyReplace,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaylistCommand {
    Create {
        name: String,
        private: bool,
    },
    Metadata {
        id: String,
        revision: String,
        name: String,
        tags: Vec<String>,
        description: String,
    },
    Tracks {
        id: String,
        revision: String,
        change: TrackChange,
        ids: Vec<String>,
    },
    Delete {
        id: String,
        revision: String,
    },
    Cover {
        id: String,
        revision: String,
        document_id: String,
    },
    Publish {
        id: String,
        revision: String,
    },
    Subscribe {
        id: String,
        subscribed: bool,
    },
    Order {
        group: PlaylistGroup,
        revision: String,
        ids: Vec<String>,
    },
}
impl PlaylistCommand {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Create { .. } => "create",
            Self::Metadata { .. } => "metadata",
            Self::Tracks { .. } => "tracks",
            Self::Delete { .. } => "delete",
            Self::Cover { .. } => "cover",
            Self::Publish { .. } => "publish",
            Self::Subscribe { .. } => "subscribe",
            Self::Order { .. } => "order",
        }
    }
    pub fn playlist_id(&self) -> Option<&str> {
        match self {
            Self::Create { .. } | Self::Order { .. } => None,
            Self::Metadata { id, .. }
            | Self::Tracks { id, .. }
            | Self::Delete { id, .. }
            | Self::Cover { id, .. }
            | Self::Publish { id, .. }
            | Self::Subscribe { id, .. } => Some(id),
        }
    }
    pub(super) fn list_change(&self) -> bool {
        matches!(
            self,
            Self::Create { .. } | Self::Delete { .. } | Self::Subscribe { .. } | Self::Order { .. }
        )
    }
    pub(super) fn conflicts(&self, other: &Self) -> bool {
        (self.list_change() && other.list_change())
            || self
                .playlist_id()
                .zip(other.playlist_id())
                .is_some_and(|(a, b)| a == b)
    }
    pub(super) fn revision(&self) -> Option<&str> {
        match self {
            Self::Metadata { revision, .. }
            | Self::Tracks { revision, .. }
            | Self::Delete { revision, .. }
            | Self::Cover { revision, .. }
            | Self::Publish { revision, .. }
            | Self::Order { revision, .. } => Some(revision),
            _ => None,
        }
    }
    pub(super) fn validate(&mut self) -> Result<()> {
        if let Some(id) = self.playlist_id() {
            identifier(&json!(id))?;
        }
        if let Some(r) = self.revision()
            && (r.len() != 64 || !r.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(Error::InvalidInput("invalid playlist revision"));
        }
        match self {
            Self::Create { name, .. } => *name = playlist_name(name)?,
            Self::Metadata {
                name,
                tags,
                description,
                ..
            } => *name = super::api::validate_metadata(name, tags, description)?,
            Self::Tracks { ids, .. } | Self::Order { ids, .. } => {
                if ids.len() > 100000 {
                    return Err(Error::InvalidInput("playlist input exceeds bound"));
                }
                let mut unique = std::collections::HashSet::new();
                for id in ids {
                    identifier(&json!(id))?;
                    if !unique.insert(id) {
                        return Err(Error::InvalidInput("duplicate playlist input id"));
                    }
                }
            }
            Self::Cover { document_id, .. } => {
                identifier(&json!(document_id))?;
            }
            _ => (),
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistPhase {
    Sending,
    Verifying,
    Uncertain,
    Confirmed,
    Rejected,
    Partial,
    Unchanged,
}
impl PlaylistPhase {
    pub fn pending(self) -> bool {
        matches!(self, Self::Sending | Self::Verifying | Self::Uncertain)
    }
}

/// Native journal: hosts must project fields instead of serializing it to UI.
#[derive(Clone, Serialize, Deserialize)]
pub struct PlaylistMutation {
    pub operation_id: String,
    pub command: PlaylistCommand,
    pub phase: PlaylistPhase,
    pub before: Value,
    pub observed: Option<Value>,
    pub playlist_id: Option<String>,
    pub created_at: f64,
    pub checked_at: Option<f64>,
    /// Only scalar response codes; never persist server error bodies/tokens.
    pub codes: BTreeMap<String, i64>,
    pub error_class: Option<String>,
}
