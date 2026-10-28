use super::{RelayMode, RelayResource, RelayResourceKind, RelaySourceKind};
use crate::{Error, Result, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PullSelection {
    Collection {
        source_type: RelaySourceKind,
        id: String,
    },
    Resources {
        resources: Vec<RelayResource>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullPlan {
    pub selection: PullSelection,
    pub current: Option<RelayResource>,
    pub position_ms: u64,
    pub mode: Option<RelayMode>,
}
impl PullPlan {
    pub fn from_data(data: &Value) -> Result<Self> {
        let source_type = match data["sourceType"].as_str() {
            Some("playlist") => Some(RelaySourceKind::Playlist),
            Some("album") => Some(RelaySourceKind::Album),
            Some("podcast") => Some(RelaySourceKind::Podcast),
            _ => None,
        };
        let current_state = &data["currentPlayState"];
        let current = match current_state
            .get("resourceId")
            .filter(|id| !id.is_null() && **id != "")
        {
            Some(id) => Some(RelayResource::new(
                id,
                match current_state["resourceType"].as_str() {
                    Some(kind) => RelayResourceKind::from_official(kind)?,
                    None if source_type == Some(RelaySourceKind::Podcast) => {
                        RelayResourceKind::Voice
                    }
                    None => RelayResourceKind::Song,
                },
            )?),
            None => None,
        };
        let mut resources = match data.get("resources") {
            Some(Value::Array(rows)) if rows.len() <= super::types::MAX_RESOURCES => rows
                .iter()
                .map(RelayResource::from_official)
                .collect::<Result<Vec<_>>>()?,
            None | Some(Value::Null) => vec![],
            _ => return Err(Error::Response("invalid relay pull resources")),
        };
        if let Some(current) = &current
            && !resources.contains(current)
        {
            resources.insert(0, current.clone());
        }
        let selection = match data["relayMode"].as_str() {
            Some("source") => {
                let source_type =
                    source_type.ok_or(Error::Response("unknown relay source type"))?;
                let source_id = data
                    .get("sourceId")
                    .filter(|id| !id.is_null() && **id != "");
                match source_id {
                    Some(id) => PullSelection::Collection {
                        source_type,
                        id: identifier(id)?,
                    },
                    None => PullSelection::Resources { resources },
                }
            }
            Some("snapshot") => PullSelection::Resources { resources },
            _ => return Err(Error::Response("unknown relay mode")),
        };
        if let PullSelection::Resources { resources } = &selection {
            super::types::validate_resources(resources)?;
        }
        // Homogeneous official cases are identical. Mixed snapshots intentionally
        // preserve per-row types instead of querying all IDs as the first type.
        let position_ms = if current
            .as_ref()
            .is_some_and(|r| r.kind == RelayResourceKind::Voice)
        {
            match current_state.get("progress") {
                None | Some(Value::Null) => 0,
                Some(value) => value
                    .as_u64()
                    .ok_or(Error::Response("invalid relay voice progress"))?,
            }
        } else {
            0
        };
        Ok(Self {
            selection,
            current,
            position_ms,
            mode: data["playMode"].as_str().and_then(RelayMode::from_official),
        })
    }
}
