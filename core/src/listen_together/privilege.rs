//! M/137, M/261 and M/3079 common access facts, independent of UI or local VIP.
use crate::{Error, Result, playback::PlaybackConstraint};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommonPrivilege {
    pub track_id: String,
    pub playable: bool,
    pub reason: Option<String>,
    pub users: Vec<String>,
    pub outer_ids: Vec<String>,
    pub preview: Option<Preview>,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preview {
    pub start: f64,
    pub end: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitedMember {
    None,
    Me,
    Other,
    Both,
    Unknown,
}
impl CommonPrivilege {
    pub fn from_rows(ids: &[String], rows: &[Value]) -> Result<Vec<Self>> {
        let mut by_id = BTreeMap::new();
        for row in rows {
            let track_id = super::model::id(&row["trackId"])?;
            let playable = row["playable"]
                .as_bool()
                .ok_or(Error::Response("missing common playable flag"))?;
            let reason = row["unplayableType"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            let users = row["unplayableUserIds"]
                .as_array()
                .ok_or(Error::Response("missing common user list"))?
                .iter()
                .map(super::model::id)
                .collect::<Result<Vec<_>>>()?;
            let outer_ids = row["unplayableOuterIds"]
                .as_array()
                .ok_or(Error::Response("missing common outer list"))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or(Error::Response("invalid common outer ID"))
                })
                .collect::<Result<Vec<_>>>()?;
            let preview = if !playable && reason.as_deref() == Some("trial") {
                let start = row["start"]
                    .as_f64()
                    .ok_or(Error::Response("common preview start missing"))?;
                let end = row["end"]
                    .as_f64()
                    .ok_or(Error::Response("common preview end missing"))?;
                if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
                    return Err(Error::Response("invalid common preview range"));
                }
                Some(Preview { start, end })
            } else {
                None
            };
            let item = Self {
                track_id: track_id.clone(),
                playable,
                reason,
                users,
                outer_ids,
                preview,
            };
            if !ids.contains(&track_id) || by_id.insert(track_id, item).is_some() {
                return Err(Error::Response("duplicate or unexpected common privilege"));
            }
        }
        ids.iter()
            .map(|id| {
                by_id
                    .remove(id)
                    .ok_or(Error::Response("incomplete common privileges"))
            })
            .collect()
    }
    pub fn can_play(&self) -> bool {
        self.playable || self.preview.is_some()
    }
    pub fn constraint(&self) -> Option<PlaybackConstraint> {
        if self.playable {
            None
        } else if let Some(p) = self.preview {
            Some(PlaybackConstraint::Preview {
                start: p.start,
                end: p.end,
            })
        } else {
            Some(PlaybackConstraint::Blocked)
        }
    }
    pub fn limited_member(&self, owner: &str) -> LimitedMember {
        if self.playable {
            LimitedMember::None
        } else {
            match self.users.as_slice() {
                [a] if a == owner => LimitedMember::Me,
                [_] => LimitedMember::Other,
                [_, _] => LimitedMember::Both,
                _ => LimitedMember::Unknown,
            }
        }
    }
}
