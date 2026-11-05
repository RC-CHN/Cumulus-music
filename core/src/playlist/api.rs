use crate::{
    Error, Result,
    catalog::{CatalogApi, SongBatch},
    error::code_matches,
    protocol::{
        api::{Api, identifier, official_text_length, packed_ids, playlist_name},
        pagination,
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};

#[derive(Clone, Copy)]
pub enum TrackOperation {
    Add,
    Delete,
    /// Historical API name for official op=update (ordering). Empty IDs do
    /// not clear a playlist; use PlaylistService's checked Clear operation.
    Replace,
}

#[derive(Debug, Serialize)]
pub struct PlaylistTracks {
    pub playlist: Value,
    #[serde(flatten)]
    pub songs: SongBatch,
    pub local_tracks: Vec<Value>,
}

#[derive(Clone)]
pub struct PlaylistApi {
    pub(super) api: Arc<dyn Api>,
    catalog: CatalogApi,
}
impl PlaylistApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self {
            catalog: CatalogApi::new(api.clone()),
            api,
        }
    }

    pub async fn list(&self, uid: &Value) -> Result<Vec<Value>> {
        pagination::all(
            self.api.as_ref(),
            "/api/user/playlist",
            json!({"uid":identifier(uid)?,"includeVideo":true}),
            "playlist",
        )
        .await
    }

    pub async fn detail(&self, pid: &Value) -> Result<Value> {
        let result = self
            .api
            .call(
                "/api/v6/playlist/detail",
                json!({"id":identifier(pid)?,"n":100000,"s":0}),
                false,
                &[200],
            )
            .await?;
        result
            .get("playlist")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or(Error::Response("missing playlist"))
    }

    /// M/2997 -> M/444.b ordinary third-party MGC playlist visit. The
    /// initial v4/dynamic reads never carry the song trial context.
    pub(crate) async fn zone_mgc_detail(&self, id: &str, owner: &str) -> Result<Value> {
        let initial = self
            .api
            .call(
                "/api/playlist/v4/detail",
                json!({"id":id,"n":0,"s":0}),
                false,
                &[200],
            )
            .await?;
        let mut playlist = initial["playlist"]
            .as_object()
            .cloned()
            .ok_or(Error::Response("missing MGC playlist"))?;
        if identifier(&initial["playlist"]["id"])? != id {
            return Err(Error::Response("MGC playlist identity mismatch"));
        }
        let creator = identifier(&initial["playlist"]["creator"]["userId"])
            .map_err(|_| Error::Response("MGC playlist creator unavailable"))?;
        if identifier(&initial["playlist"]["userId"]).is_ok_and(|owner| owner != creator) {
            return Err(Error::Response("MGC playlist creator identity mismatch"));
        }
        if creator == owner || initial["playlist"]["specialType"].as_i64() == Some(100) {
            return Err(Error::State(
                "owned and special MGC playlist details are not supported",
            ));
        }
        match self
            .api
            .call(
                "/api/playlist/detail/dynamic",
                json!({"id":id}),
                false,
                &[200],
            )
            .await
        {
            Ok(dynamic) => {
                for (source, target) in [
                    ("bookedCount", "subscribedCount"),
                    ("commentCount", "commentCount"),
                    ("gradeStatus", "gradeStatus"),
                    ("playCount", "playCount"),
                    ("shareCount", "shareCount"),
                    ("subscribed", "subscribed"),
                ] {
                    if let Some(value) = dynamic.get(source) {
                        playlist.insert(target.into(), value.clone());
                    }
                }
            }
            Err(error)
                if error.is_service_code(301)
                    || matches!(error, Error::StaleResponse | Error::CredentialChanged) =>
            {
                return Err(error);
            }
            Err(_) => {}
        }
        Ok(Value::Object(playlist))
    }

    pub async fn tracks(&self, pid: &Value) -> Result<PlaylistTracks> {
        let playlist = self.detail(pid).await?;
        let tracks = playlist["trackIds"]
            .as_array()
            .ok_or(Error::Response("missing authoritative trackIds"))?;
        let mut ids = vec![];
        let mut local_tracks = vec![];
        for track in tracks {
            if track["v"].as_i64() == Some(-1) {
                local_tracks.push(track.clone());
            } else {
                ids.push(track["id"].clone());
            }
        }
        let songs = self.catalog.songs(&ids, 500).await?;
        Ok(PlaylistTracks {
            playlist,
            songs,
            local_tracks,
        })
    }

    pub async fn create(&self, uid: &Value, name: &str, private: bool) -> Result<String> {
        let result = self.api.call("/api/playlist/create",json!({"uid":identifier(uid)?,"name":playlist_name(name)?,"privacy":if private {10} else {0}}),false,&[200]).await?;
        // An acknowledged mutation without an ID has unknown outcome. Never
        // issue a second create; the caller must reconcile against the list.
        result
            .get("id")
            .filter(|v| identifier(v).is_ok())
            .or_else(|| result.get("playlist").and_then(|v| v.get("id")))
            .and_then(|v| identifier(v).ok())
            .ok_or(Error::Response(
                "creation response has no playlist id; reconcile before retrying",
            ))
    }

    pub async fn rename(&self, pid: &Value, name: &str) -> Result<Value> {
        self.api
            .call(
                "/api/playlist/update/name",
                json!({"id":identifier(pid)?,"name":playlist_name(name)?}),
                false,
                &[200],
            )
            .await
    }

    pub async fn describe(&self, pid: &Value, description: &str) -> Result<Value> {
        self.api
            .call(
                "/api/playlist/desc/update",
                json!({"id":identifier(pid)?,"desc":description}),
                false,
                &[200],
            )
            .await
    }

    pub async fn update_metadata(
        &self,
        pid: &Value,
        name: &str,
        tags: &[String],
        description: &str,
    ) -> Result<Value> {
        let name = validate_metadata(name, tags, description)?;
        let pid = identifier(pid)?;
        // M/444 h + M/48: each child object is encoded as JSON text. The batch
        // is not atomic. Successful children must be retained for reconciliation.
        let children = [
            ("/api/playlist/update/name", json!({"id":pid,"name":name})),
            (
                "/api/playlist/tags/update",
                json!({"id":pid,"tags":tags.join(";")}),
            ),
            (
                "/api/playlist/desc/update",
                json!({"id":pid,"desc":description}),
            ),
        ];
        let mut payload = json!({});
        for (path, value) in &children {
            payload[*path] = serde_json::to_string(value)?.into();
        }
        let result = self.api.call("/api/batch", payload, false, &[200]).await?;
        let failed: Vec<_> = children
            .iter()
            .filter(|(path, _)| {
                !result[*path].is_object() || !code_matches(&result[*path]["code"], 200)
            })
            .map(|(path, _)| (*path).to_owned())
            .collect();
        if !failed.is_empty() {
            return Err(Error::Batch {
                responses: result,
                failed,
            });
        }
        Ok(result)
    }

    pub async fn change_tracks(
        &self,
        pid: &Value,
        ids: &[Value],
        operation: TrackOperation,
    ) -> Result<Value> {
        let (op, imme) = match operation {
            TrackOperation::Add => ("add", true),
            TrackOperation::Delete => ("del", true),
            TrackOperation::Replace => ("update", false),
        };
        let mut payload = json!({"pid":identifier(pid)?,"trackIds":packed_ids(ids)?,"op":op});
        if imme {
            payload["imme"] = json!(true);
        }
        self.api
            .call("/api/v1/playlist/manipulate/tracks", payload, false, &[200])
            .await
    }

    pub async fn delete(&self, pid: &Value) -> Result<Value> {
        self.api
            .call(
                "/api/playlist/delete",
                json!({"pid":identifier(pid)?}),
                false,
                &[200],
            )
            .await
    }
}

pub(super) fn validate_metadata(name: &str, tags: &[String], description: &str) -> Result<String> {
    let name = playlist_name(name)?;
    if official_text_length(description) > 1000 {
        return Err(Error::InvalidInput(
            "description exceeds weighted length 1000",
        ));
    }
    let mut unique = HashSet::new();
    if tags.len() > 3
        || tags
            .iter()
            .any(|t| t.is_empty() || t.contains(';') || !unique.insert(t))
    {
        return Err(Error::InvalidInput(
            "expected at most three unique labels without semicolons",
        ));
    }
    Ok(name)
}
