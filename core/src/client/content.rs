use super::{MusicClient, dto::*, mapping};
use crate::{
    Error, Result,
    account::resources::{AccountResources, ResourceKind},
    catalog::{CatalogApi, SearchOptions},
    playlist::PlaylistApi,
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub(super) fn playlist_track_revision(id: &str, detail: &Value) -> Result<String> {
    let ids = detail["trackIds"]
        .as_array()
        .ok_or(Error::Response("missing trackIds"))?;
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&json!([
        id, ids
    ]))?)))
}

pub(super) async fn playlist_track_page(
    api: Arc<dyn Api>,
    uid: &str,
    id: &str,
    detail: &Value,
    page: &PageRequest,
    trial_mode: Option<i32>,
) -> Result<TrackPage> {
    let playlist = mapping::playlist(detail, uid)?;
    let ids = detail["trackIds"]
        .as_array()
        .ok_or(Error::Response("missing trackIds"))?;
    let revision = playlist_track_revision(id, detail)?;
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
    let songs = CatalogApi::new(api)
        .songs_with_trial(
            &wanted,
            if trial_mode.is_some() { 500 } else { 100 },
            trial_mode,
        )
        .await?;
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
                artist_refs: vec![],
                album: String::new(),
                album_id: None,
                cover_url: None,
                duration_ms: 0,
                fee: 0,
                missing: true,
                local,
            },
        });
    }
    Ok(TrackPage {
        daily: None,
        style_tags: None,
        revision: Some(revision),
        more: page.offset.saturating_add(tracks.len()) < ids.len(),
        tracks,
        total: ids.len(),
        offset: page.offset,
        playlist: Some(playlist),
    })
}

impl MusicClient {
    pub async fn playlists(&self) -> Result<AccountData<Vec<PlaylistView>>> {
        let (token, (value, rows, api, revision)) = self
            .session
            .run(|api, uid| async move {
                let revision = api.verified_pc_experiment_revision().ok().flatten();
                let rows = PlaylistApi::new(api.clone()).list(&json!(uid)).await?;
                let value = rows
                    .iter()
                    .map(|p| mapping::playlist(p, &uid))
                    .collect::<Result<Vec<_>>>()?;
                Ok((value, rows, api, revision))
            })
            .await?;
        if let Some(revision) = revision {
            self.remember_zone_playlist_membership(&token, &api, &revision, &rows)?;
        }
        self.remember_queue_more_memberships(&token, &rows)?;
        self.remember_intelligence_likes(
            &token,
            value
                .iter()
                .find(|p| p.favorite && p.owned)
                .map(|p| p.id.clone()),
        )?;
        Ok(AccountData {
            epoch: token.generation.to_string(),
            value,
        })
    }

    /// M/3132's collector RPC uses page indexes, not item offsets. Only the
    /// current account's editable targets are returned to the WebView.
    pub async fn playlist_collect_page(
        &self,
        epoch: &str,
        sort: PlaylistCollectSort,
        page: u32,
    ) -> Result<AccountData<PlaylistCollectPage>> {
        const LIMIT: usize = 10;
        const LAST_PAGE: u32 = 999;
        if page > LAST_PAGE {
            return Err(Error::InvalidInput("invalid playlist collection page"));
        }
        let token = self.player_token(epoch)?;
        let value = self
            .session
            .run_for(&token, |api, uid| async move {
                let response = api
                    .call(
                        "/api/user/playlist/v2",
                        json!({"uid":uid,"offset":page,"limit":LIMIT,"sortType":sort.wire()}),
                        false,
                        &[200],
                    )
                    .await?;
                let rows = match &response["playlist"] {
                    Value::Null => &[][..],
                    Value::Array(rows) if rows.len() <= LIMIT => rows.as_slice(),
                    _ => return Err(Error::Response("invalid playlist collection page")),
                };
                let more = match &response["more"] {
                    Value::Null => false,
                    Value::Bool(more) => *more,
                    _ => return Err(Error::Response("invalid playlist collection cursor")),
                };
                if more && (rows.is_empty() || page == LAST_PAGE) {
                    return Err(Error::Response("invalid playlist collection continuation"));
                }
                let mut seen = std::collections::HashSet::with_capacity(rows.len());
                let mut playlists = Vec::new();
                for row in rows {
                    let item = mapping::playlist(row, &uid)?;
                    if !seen.insert(item.id.clone()) {
                        return Err(Error::Response("duplicate playlist collection row"));
                    }
                    if item.owned && !item.favorite {
                        playlists.push(item);
                    }
                }
                Ok(PlaylistCollectPage {
                    playlists,
                    next_page: more.then_some(page + 1),
                })
            })
            .await?;
        Ok(AccountData {
            epoch: token.generation.to_string(),
            value,
        })
    }

    pub async fn playlist_subscribers(
        &self,
        epoch: &str,
        id: &str,
        offset: u32,
    ) -> Result<AccountData<PlaylistSubscribersPage>> {
        const LIMIT: u32 = 24;
        let token = self.player_token(epoch)?;
        let id = identifier(&json!(id))?;
        let next = offset
            .checked_add(LIMIT)
            .filter(|_| offset.is_multiple_of(LIMIT))
            .ok_or(Error::InvalidInput("invalid subscriber offset"))?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                let response = api
                    .call(
                        "/api/playlist/subscribers",
                        json!({"id":id,"limit":LIMIT,"offset":offset}),
                        false,
                        &[200],
                    )
                    .await?;
                let rows = response["subscribers"]
                    .as_array()
                    .filter(|rows| rows.len() <= LIMIT as usize)
                    .ok_or(Error::Response("invalid playlist subscriber page"))?;
                let more = response["more"]
                    .as_bool()
                    .ok_or(Error::Response("missing playlist subscriber pagination"))?;
                if more && rows.is_empty() {
                    return Err(Error::Response(
                        "empty playlist subscriber page with more results",
                    ));
                }
                let subscribers = rows
                    .iter()
                    .map(|row| {
                        let nickname = ["nickname", "nickName", "userName"]
                            .iter()
                            .filter_map(|key| row[*key].as_str())
                            .find(|name| !name.trim().is_empty())
                            .ok_or(Error::Response("missing playlist subscriber name"))?;
                        Ok(PlaylistSubscriberView {
                            user_id: identifier(&row["userId"]).ok(),
                            nickname: nickname.chars().take(128).collect(),
                            avatar_url: mapping::artwork(&row["avatarUrl"]),
                            signature: row["signature"]
                                .as_str()
                                .unwrap_or_default()
                                .chars()
                                .take(512)
                                .collect(),
                        })
                    })
                    .collect::<Result<_>>()?;
                Ok(PlaylistSubscribersPage {
                    subscribers,
                    previous_offset: offset.checked_sub(LIMIT),
                    next_offset: more.then_some(next),
                })
            })
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
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
                playlist_track_page(api, &uid, &id, &detail, &page, None).await
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
                    daily: None,
                    style_tags: None,
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

    pub async fn liked_ids(&self) -> Result<AccountData<Vec<String>>> {
        let token = self.session.token()?;
        let value = AccountResources::new(self.session.clone())?
            .refresh_for(&token, ResourceKind::Liked)
            .await?;
        let value: Vec<String> = serde_json::from_value(value)?;
        self.session.with_token(&token, |_| Ok(()))?;
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
