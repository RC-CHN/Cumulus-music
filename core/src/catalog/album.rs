//! Official album page: M/1204 in hybrid_162, orchestrated by M/2998.
use super::{CatalogApi, privileges::merge_song_privileges};
use crate::{Error, Result, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlbumDetailSource {
    V3,
    V4,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumReadState {
    pub source: AlbumDetailSource,
    pub privileges_refreshed: bool,
    /// None distinguishes a failed auxiliary read from a confirmed false.
    pub digital_album_tag: Option<bool>,
}

pub struct AlbumDetails {
    /// Native metadata only; must be projected before crossing the IPC boundary.
    pub response: Value,
    pub read: AlbumReadState,
}

impl CatalogApi {
    /// M/404 -> M/15 da: playback re-reads v3 and requires album privileges.
    /// This path does not request page dynamic/exposure metadata or supplement
    /// missing tracks. The first matching privilege is used (Array.find).
    pub async fn album_playback_tracks(&self, id: &Value) -> Result<Vec<Value>> {
        let id = identifier(id)?;
        let response = self.album(&json!(id)).await?;
        validate_detail(&response, &id)?;
        let rights = self
            .api
            .call("/api/album/privilege", json!({"id":id}), false, &[200])
            .await?;
        let rights = rights["data"]
            .as_array()
            .ok_or(Error::Response("missing album playback privileges"))?;
        let mut by_id = std::collections::BTreeMap::new();
        for right in rights {
            by_id
                .entry((right["id"].is_string(), identifier(&right["id"])?))
                .or_insert(right);
        }
        let mut songs = response["songs"].as_array().unwrap().clone();
        for song in &mut songs {
            if let Some(right) = by_id.get(&(song["id"].is_string(), identifier(&song["id"])?)) {
                song["privilege"] = (*right).clone();
            } else {
                song.as_object_mut().unwrap().remove("privilege");
            }
        }
        Ok(songs)
    }
    /// The page requires dynamic metadata, but retains songs if the separate
    /// privilege read fails. No collection, payment or exposure write is sent.
    pub async fn album_details(&self, id: &Value) -> Result<AlbumDetails> {
        let id = identifier(id)?;
        let (mut response, source) = match self.album(&json!(id)).await {
            Ok(value) => (value, AlbumDetailSource::V3),
            Err(_) => (
                self.api
                    .call("/api/album/v4/detail", json!({"id":id}), false, &[200])
                    .await?,
                AlbumDetailSource::V4,
            ),
        };
        validate_detail(&response, &id)?;
        let dynamic = self
            .api
            .call("/api/album/detail/dynamic", json!({"id":id}), false, &[200])
            .await?;
        let info = response["album"]["info"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        let album = response["album"].as_object_mut().unwrap();
        album.extend(info);
        for field in [
            "commentCount",
            "likedCount",
            "shareCount",
            "subCount",
            "isSub",
            "subTime",
        ] {
            // Object spread followed by explicit fields; an omitted dynamic
            // property must not leave an old static collection value behind.
            album.remove(field);
            if let Some(value) = dynamic.get(field) {
                album.insert(field.into(), value.clone());
            }
        }
        validate_detail(&response, &id)?;
        let privileges = self
            .api
            .call("/api/album/privilege", json!({"id":id}), false, &[200])
            .await;
        let merged = privileges.and_then(|value| {
            let rows = match &value["data"] {
                Value::Null => &[][..],
                Value::Array(rows) => rows.as_slice(),
                _ => return Err(Error::Response("invalid album privileges")),
            };
            merge_song_privileges(response["songs"].as_array().unwrap(), rows)
        });
        let privileges_refreshed = merged.is_ok();
        if let Ok(songs) = merged {
            response["songs"] = json!(songs);
        }
        let digital_album_tag = self
            .api
            .call(
                "/api/resource-exposure/config",
                json!({"resourcePosition":"album","resourceId":id,"rexposureRecords":[]}),
                false,
                &[200],
            )
            .await
            .ok()
            .map(|value| value["data"]["resourceType"] == "common");
        Ok(AlbumDetails {
            response,
            read: AlbumReadState {
                source,
                privileges_refreshed,
                digital_album_tag,
            },
        })
    }
}

fn validate_detail(response: &Value, id: &str) -> Result<()> {
    if super::album_identifier(&response["album"])? != id {
        return Err(Error::Response("album identity mismatch"));
    }
    for song in response["songs"]
        .as_array()
        .ok_or(Error::Response("missing album songs"))?
    {
        identifier(&song["id"])?;
    }
    Ok(())
}
