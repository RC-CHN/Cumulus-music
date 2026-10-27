use super::{PlayCommand, SharedMode, Version};
use crate::{Error, Result, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// Room IDs are opaque (the live response uses 43 characters), not song IDs.
pub(super) fn opaque(value: &str) -> Result<&str> {
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(Error::InvalidInput(
            "invalid together room/channel identifier",
        ));
    }
    Ok(value)
}
pub(super) fn id(value: &Value) -> Result<String> {
    identifier(value).map_err(|_| Error::Response("invalid together identity"))
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub user_id: String,
    pub nickname: String,
    pub avatar_url: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Room {
    pub room_id: String,
    pub creator_id: String,
    pub chat_room_id: String,
    pub channel_id: String,
    pub members: Vec<Member>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomRole {
    Waiting,
    TogetherOwner,
    Together,
}
impl Room {
    pub fn from_official(v: &Value) -> Result<Self> {
        let room_id = opaque(
            v["roomId"]
                .as_str()
                .ok_or(Error::Response("missing room ID"))?,
        )?
        .into();
        let channel = match &v["agoraChannelId"] {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => return Err(Error::Response("missing RTC channel")),
        };
        opaque(&channel)?;
        let members = v["roomUsers"]
            .as_array()
            .ok_or(Error::Response("room members missing"))?
            .iter()
            .map(|m| {
                Ok(Member {
                    user_id: id(&m["userId"])?,
                    nickname: m["nickname"].as_str().unwrap_or("").into(),
                    avatar_url: m["avatarUrl"].as_str().map(str::to_owned),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut seen = BTreeSet::new();
        if members.is_empty() || members.iter().any(|m| !seen.insert(&m.user_id)) {
            return Err(Error::Response("empty or duplicate together members"));
        }
        Ok(Self {
            room_id,
            creator_id: id(&v["creatorId"])?,
            chat_room_id: id(&v["chatRoomId"])?,
            channel_id: channel,
            members,
        })
    }
    pub fn role_for(&self, owner: &str) -> Result<RoomRole> {
        if !self.members.iter().any(|m| m.user_id == owner) {
            return Err(Error::Response("together room excludes current account"));
        }
        Ok(if self.creator_id == owner {
            if self.members.len() == 1 {
                RoomRole::Waiting
            } else {
                RoomRole::TogetherOwner
            }
        } else {
            RoomRole::Together
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedPlaylist {
    pub display: Vec<String>,
    /// The service legitimately omits randomList in ORDER_LOOP mode.
    pub random: Option<Vec<String>>,
    pub mode: SharedMode,
    pub version: Vec<Version>,
    pub changed: bool,
    pub replace: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TogetherSnapshot {
    pub playlist: Option<SharedPlaylist>,
    pub play_command: Option<PlayCommand>,
}
impl TogetherSnapshot {
    pub fn from_official(v: &Value) -> Result<Self> {
        if !v.is_object() {
            return Err(Error::Response("invalid together snapshot"));
        }
        let playlist = if v["playlist"].is_null() {
            None
        } else {
            let p = &v["playlist"];
            let display = ids(&p["displayList"]["result"])?;
            let random = if p["randomList"].is_null() {
                None
            } else {
                Some(ids(&p["randomList"]["result"])?)
            };
            let mode: SharedMode = serde_json::from_value(p["playMode"].clone())?;
            // The live service retains its previous random ordering after a
            // DELETE in ORDER_LOOP. It is authoritative only in RANDOM mode.
            if mode == SharedMode::Random
                && let Some(random) = &random
                && random.iter().collect::<BTreeSet<_>>() != display.iter().collect()
            {
                return Err(Error::Response("shared queue permutation mismatch"));
            }
            Some(SharedPlaylist {
                display,
                random,
                mode,
                version: Version::parse(&p["version"])?,
                changed: p["displayList"]["changed"]
                    .as_bool()
                    .ok_or(Error::Response("queue changed flag missing"))?,
                replace: p["replace"]
                    .as_bool()
                    .ok_or(Error::Response("queue replace flag missing"))?,
            })
        };
        let play_command = if v["playCommand"].is_null() {
            None
        } else {
            Some(PlayCommand::from_official(&v["playCommand"])?)
        };
        Ok(Self {
            playlist,
            play_command,
        })
    }
}
pub(super) fn ids(value: &Value) -> Result<Vec<String>> {
    let rows = value
        .as_array()
        .ok_or(Error::Response("missing shared queue IDs"))?;
    if rows.len() > 1000 {
        return Err(Error::Response("shared queue exceeds official window"));
    }
    let rows = rows.iter().map(id).collect::<Result<Vec<_>>>()?;
    if rows.iter().collect::<BTreeSet<_>>().len() != rows.len() {
        return Err(Error::Response("duplicate shared queue IDs"));
    }
    Ok(rows)
}
