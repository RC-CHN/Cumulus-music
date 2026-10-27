use super::{MusicClient, dto::*, mapping};
use crate::{
    Error, Result,
    account::{
        AccountApi,
        resources::{AccountResources, ResourceKind},
    },
    catalog::{CatalogApi, SearchOptions},
    playlist::PlaylistApi,
    protocol::api::identifier,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

impl MusicClient {
    pub async fn playlists(&self) -> Result<AccountData<Vec<PlaylistView>>> {
        let (token, value) = self
            .session
            .run(|api, uid| async move {
                PlaylistApi::new(api)
                    .list(&json!(uid))
                    .await?
                    .iter()
                    .map(|p| mapping::playlist(p, &uid))
                    .collect::<Result<Vec<_>>>()
            })
            .await?;
        Ok(AccountData {
            epoch: token.generation.to_string(),
            value,
        })
    }

    pub async fn playlist_page(
        &self,
        id: &str,
        page: PageRequest,
    ) -> Result<AccountData<TrackPage>> {
        mapping::page(page.offset, page.limit)?;
        let id = identifier(&json!(id))?;
        let (token, value) = self
            .session
            .run(|api, uid| async move {
                // Read authoritative trackIds, never assume the preview `tracks`
                // contains the complete playlist. Page song detail separately.
                let detail = PlaylistApi::new(api.clone()).detail(&json!(id)).await?;
                let playlist = mapping::playlist(&detail, &uid)?;
                let ids = detail["trackIds"]
                    .as_array()
                    .ok_or(Error::Response("missing trackIds"))?;
                let revision = hex::encode(Sha256::digest(serde_json::to_vec(&json!([id, ids]))?));
                if page
                    .revision
                    .as_ref()
                    .is_some_and(|expected| expected != &revision)
                {
                    return Err(Error::QueueChanged);
                }
                let slice: Vec<_> = ids.iter().skip(page.offset).take(page.limit).collect();
                let wanted: Vec<_> = slice
                    .iter()
                    .filter(|v| v["v"].as_i64() != Some(-1))
                    .map(|v| v["id"].clone())
                    .collect();
                let songs = CatalogApi::new(api).songs(&wanted, 100).await?;
                let mut returned = songs.songs.into_iter();
                let mut tracks = Vec::with_capacity(slice.len());
                for item in slice {
                    let local = item["v"].as_i64() == Some(-1);
                    let song = if local {
                        None
                    } else {
                        returned.next().flatten()
                    };
                    tracks.push(match song {
                        Some(song) => mapping::track(&song)?,
                        None => TrackView {
                            id: identifier(&item["id"])?,
                            voice_id: None,
                            name: if local {
                                "本地音乐"
                            } else {
                                "暂不可用的歌曲"
                            }
                            .into(),
                            artists: vec![],
                            album: String::new(),
                            cover_url: None,
                            duration_ms: 0,
                            fee: 0,
                            missing: true,
                            local,
                        },
                    });
                }
                Ok(TrackPage {
                    revision: Some(revision),
                    more: page.offset.saturating_add(tracks.len()) < ids.len(),
                    tracks,
                    total: ids.len(),
                    offset: page.offset,
                    playlist: Some(playlist),
                })
            })
            .await?;
        Ok(AccountData {
            epoch: token.generation.to_string(),
            value,
        })
    }

    pub async fn search_songs(
        &self,
        keyword: &str,
        page: PageRequest,
    ) -> Result<AccountData<TrackPage>> {
        mapping::page(page.offset, page.limit)?;
        if keyword.chars().count() > 256 {
            return Err(Error::InvalidInput("search text too long"));
        }
        let (token, value) = self
            .session
            .run(|api, _| async move {
                let response = CatalogApi::new(api)
                    .search(
                        keyword,
                        SearchOptions {
                            kind: 1,
                            limit: page.limit,
                            offset: page.offset,
                        },
                    )
                    .await?;
                let result = &response["result"];
                let total = result["songCount"]
                    .as_u64()
                    .ok_or(Error::Response("missing search total"))?
                    as usize;
                let tracks = match result["songs"].as_array() {
                    Some(rows) => rows
                        .iter()
                        .map(mapping::track)
                        .collect::<Result<Vec<_>>>()?,
                    None if total == 0 => vec![],
                    None => return Err(Error::Response("missing search songs")),
                };
                Ok(TrackPage {
                    revision: None,
                    more: page.offset.saturating_add(tracks.len()) < total,
                    tracks,
                    total,
                    offset: page.offset,
                    playlist: None,
                })
            })
            .await?;
        Ok(AccountData {
            epoch: token.generation.to_string(),
            value,
        })
    }

    pub async fn daily_songs(&self) -> Result<AccountData<TrackPage>> {
        let (token, value) = self
            .session
            .run(|api, _| async move {
                let response = CatalogApi::new(api).daily().await?;
                let tracks = response["data"]["dailySongs"]
                    .as_array()
                    .ok_or(Error::Response("missing daily songs"))?
                    .iter()
                    .map(mapping::track)
                    .collect::<Result<Vec<_>>>()?;
                Ok(TrackPage {
                    revision: None,
                    total: tracks.len(),
                    tracks,
                    offset: 0,
                    more: false,
                    playlist: None,
                })
            })
            .await?;
        Ok(AccountData {
            epoch: token.generation.to_string(),
            value,
        })
    }

    pub async fn liked_ids(&self) -> Result<AccountData<Vec<String>>> {
        let (token, value) = self
            .session
            .run(|api, uid| async move { AccountApi::new(api).liked_ids(&json!(uid)).await })
            .await?;
        Ok(AccountData {
            epoch: token.generation.to_string(),
            value,
        })
    }

    pub async fn set_liked(
        &self,
        epoch: &str,
        id: &str,
        liked: bool,
    ) -> Result<AccountData<LikeResult>> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        let id = identifier(&json!(id))?;
        let result = AccountResources::new(self.session.clone())?
            .set_membership_for(&token, ResourceKind::Liked, &json!(id), liked)
            .await?;
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(AccountData {
            epoch: token.generation.to_string(),
            value: LikeResult {
                id,
                phase: result.phase,
                observed: result
                    .observed
                    .as_ref()
                    .or_else(|| {
                        matches!(
                            result.phase,
                            crate::account::resources::MutationPhase::Unchanged
                        )
                        .then_some(&result.before)
                    })
                    .and_then(Value::as_bool),
            },
        })
    }
}
