use super::{PlaylistApi, PlaylistGroup};
use crate::{Error, Result, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Clone, Serialize, Deserialize)]
pub struct PlaylistSummary {
    pub id: String,
    pub owner: String,
    pub name: String,
    pub private: bool,
    pub favorite: bool,
}
impl PlaylistSummary {
    pub(super) fn parse(v: &Value) -> Result<Self> {
        Ok(Self {
            id: identifier(&v["id"])?,
            owner: identifier(&v["userId"]).or_else(|_| identifier(&v["creator"]["userId"]))?,
            name: v["name"]
                .as_str()
                .ok_or(Error::Response("missing playlist name"))?
                .into(),
            private: match v["privacy"].as_i64() {
                Some(10) => true,
                Some(0) => false,
                _ => return Err(Error::Response("missing playlist privacy")),
            },
            favorite: v["specialType"].as_i64() == Some(5),
        })
    }
}
pub(super) fn revision<T: Serialize>(value: &T) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PlaylistList {
    pub playlists: Vec<PlaylistSummary>,
    pub revision: String,
}
impl PlaylistList {
    pub(super) async fn read(api: &PlaylistApi, uid: &str) -> Result<Self> {
        let playlists = api
            .list(&json!(uid))
            .await?
            .iter()
            .map(PlaylistSummary::parse)
            .collect::<Result<Vec<_>>>()?;
        let mut unique = std::collections::HashSet::new();
        if playlists.iter().any(|p| !unique.insert(&p.id)) {
            return Err(Error::Response("duplicate playlist list entry"));
        }
        Ok(Self {
            revision: revision(&playlists)?,
            playlists,
        })
    }
    pub fn group(&self, uid: &str, group: PlaylistGroup) -> Vec<String> {
        self.playlists
            .iter()
            .filter(|p| (p.owner == uid) == (group == PlaylistGroup::Created))
            .map(|p| p.id.clone())
            .collect()
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PlaylistSnapshot {
    #[serde(flatten)]
    pub summary: PlaylistSummary,
    pub description: String,
    pub tags: Vec<String>,
    pub cover_id: Option<String>,
    #[serde(default)]
    pub cover_url: Option<String>,
    pub tracks: Vec<String>,
    pub has_local: bool,
    pub revision: String,
}
impl PlaylistSnapshot {
    pub(super) async fn read(api: &PlaylistApi, id: &str) -> Result<Self> {
        let value = api.detail(&json!(id)).await?;
        let summary = PlaylistSummary::parse(&value)?;
        if summary.id != id {
            return Err(Error::Response("playlist identity mismatch"));
        }
        let entries = value["trackIds"]
            .as_array()
            .ok_or(Error::Response("missing complete playlist tracks"))?;
        let tags = value["tags"]
            .as_array()
            .ok_or(Error::Response("missing playlist tags"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or(Error::Response("invalid playlist tag"))
            })
            .collect::<Result<Vec<_>>>()?;
        let tracks = entries
            .iter()
            .map(|t| identifier(&t["id"]))
            .collect::<Result<Vec<_>>>()?;
        let has_local = entries.iter().any(|t| t["v"].as_i64() == Some(-1));
        let description = match &value["description"] {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            _ => return Err(Error::Response("invalid playlist description")),
        };
        let cover_id = identifier(&value["coverImgId_str"])
            .or_else(|_| identifier(&value["coverImgId"]))
            .ok();
        let revision = revision(&json!([&summary, &description, &tags, &cover_id, entries]))?;
        Ok(Self {
            summary,
            description,
            tags,
            cover_id,
            cover_url: value["coverImgUrl"].as_str().map(str::to_owned),
            tracks,
            has_local,
            revision,
        })
    }
    pub(super) fn writable(&self, uid: &str) -> Result<()> {
        if self.summary.owner != uid || self.summary.favorite {
            return Err(Error::InvalidInput(
                "this operation requires a regular owned playlist",
            ));
        }
        Ok(())
    }
}
