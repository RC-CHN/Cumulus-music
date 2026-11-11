//! Assemble the owned playlist using the service order and the device overlay.
use super::{
    MusicClient, mapping, missing_playlist_row, playlist_rows, playlist_track_page,
    playlist_track_revision,
};
use crate::{
    Error, Result,
    account::SessionToken,
    client::playlist_offline::{invalidates_cache, may_fallback},
    client::{dto::*, local::view},
    playlist::{PlaylistApi, PlaylistLocalRow, PlaylistLocalStore},
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

pub(super) enum PlaylistPageSlot<'a> {
    Service(&'a Value),
    Local(Box<TrackView>),
}

pub(in crate::client) struct OnlinePlaylistPage {
    pub(in crate::client) page: TrackPage,
    pub(in crate::client) api: Arc<dyn Api>,
    pub(in crate::client) membership_generation: Option<u64>,
}

impl MusicClient {
    pub async fn playlist_page_at(
        &self,
        id: &str,
        page: PageRequest,
        local_revision: Option<String>,
    ) -> Result<AccountData<TrackPage>> {
        self.playlist_page_at_inner(id, page, local_revision, false)
            .await
    }

    /// Observe only the ordinary displayed playlist visit. Background reads
    /// for batch and local membership must not change its menu receipts.
    pub async fn playlist_song_menu_page_at(
        &self,
        id: &str,
        page: PageRequest,
        local_revision: Option<String>,
    ) -> Result<AccountData<TrackPage>> {
        self.playlist_page_at_inner(id, page, local_revision, true)
            .await
    }

    async fn playlist_page_at_inner(
        &self,
        id: &str,
        page: PageRequest,
        local_revision: Option<String>,
        song_menu: bool,
    ) -> Result<AccountData<TrackPage>> {
        mapping::page(page.offset, page.limit)?;
        let id = identifier(&json!(id))?;
        let token = self.session.token()?;
        let observation = if song_menu {
            self.playlist_song_comments.begin(&token, &id, &page)
        } else {
            None
        };
        let (mut page, live) = match self
            .online_playlist_page(&token, &id, &page, local_revision.as_deref(), None)
            .await
        {
            Ok(online) => (
                online.page,
                Some((online.api, online.membership_generation)),
            ),
            Err(error) if may_fallback(&error) => {
                match self
                    .offline_playlist_page(&token, &id, &page, local_revision.as_deref())
                    .await
                {
                    Ok(Some(page)) => {
                        self.playlist_song_comments.retire(observation);
                        (page, None)
                    }
                    Ok(None) => return Err(error),
                    Err(cache_error) => return Err(cache_error),
                }
            }
            Err(error) => {
                self.playlist_song_comments.retire(observation);
                return Err(error);
            }
        };
        if observation.is_some()
            && let Some((api, _)) = &live
        {
            self.session.require_api(&token, api)?;
        }
        self.session.with_token(&token, |_| {
            // A sorted view uses its own source receipt. Keep its observed
            // prefix free of menu capabilities from the ordinary paged visit.
            self.playlist_duration.observe(
                &token,
                &page,
                live.as_ref().map(|(api, generation)| (api, *generation)),
            );
            if let Some((api, generation)) = live {
                self.playlist_song_comments.observe(
                    observation,
                    &token,
                    &api,
                    generation,
                    &mut page,
                );
            }
            Ok(AccountData {
                epoch: token.generation.to_string(),
                value: page,
            })
        })
    }

    pub(in crate::client) async fn online_playlist_page(
        &self,
        token: &SessionToken,
        id: &str,
        page: &PageRequest,
        local_revision: Option<&str>,
        duration_limit: Option<usize>,
    ) -> Result<OnlinePlaylistPage> {
        let (api, uid, detail) = match self
            .session
            .run_for(token, |api, uid| async {
                let detail = PlaylistApi::new(api.clone()).detail(&json!(id)).await?;
                Ok((api, uid, detail))
            })
            .await
        {
            Ok(data) => data,
            Err(error) => {
                if invalidates_cache(&error) {
                    self.session.with_token(token, |_| {
                        self.playlist_duration.forget(token, id);
                        Ok(())
                    })?;
                    let _ = self.invalidate_offline_playlist(token, id);
                }
                return Err(error);
            }
        };
        let mut playlist = mapping::playlist(&detail, &uid)?;
        if playlist.id != id {
            return Err(Error::Response("playlist identity mismatch"));
        }
        if duration_limit.is_some_and(|max| {
            detail["trackIds"]
                .as_array()
                .is_some_and(|rows| rows.len() > max)
        }) {
            return Err(Error::InvalidInput("playlist too large to sort"));
        }
        // TrackList -> M/2997 -> M/120 includes trialMode=-1 even when the
        // ordinary route has no specific trial. The normal page remains unchanged.
        let trial_mode = duration_limit.map(|_| -1);
        if !playlist.owned || playlist.favorite {
            let _ = self.invalidate_offline_playlist(token, id);
            if local_revision.is_some() {
                return Err(Error::QueueChanged);
            }
            let page = self
                .session
                .run_for(token, |current, _| async {
                    self.session.require_api(token, &api)?;
                    playlist_track_page(current, &uid, id, &detail, page, trial_mode, None).await
                })
                .await?;
            return Ok(OnlinePlaylistPage {
                page,
                api,
                membership_generation: None,
            });
        }
        let result = {
            let revision = playlist_track_revision(id, &detail)?;
            if (page.offset > 0 && (page.revision.is_none() || local_revision.is_none()))
                || page.revision.as_ref().is_some_and(|r| r != &revision)
            {
                return Err(Error::QueueChanged);
            }
            let store = PlaylistLocalStore::new(self.session.clone())?;
            let membership = store.project(token, id, &detail)?;
            if duration_limit.is_some_and(|max| membership.rows.len() > max) {
                return Err(Error::InvalidInput("playlist too large to sort"));
            }
            let generation = membership.generation();
            if local_revision.is_some_and(|r| r != membership.revision) {
                return Err(Error::QueueChanged);
            }
            let tracks = detail["trackIds"]
                .as_array()
                .ok_or(Error::Response("missing trackIds"))?;
            let slice = membership
                .rows
                .iter()
                .skip(page.offset)
                .take(page.limit)
                .collect::<Vec<_>>();
            let local = slice
                .iter()
                .filter_map(|row| match row {
                    PlaylistLocalRow::Local(reference) => Some(reference.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let mut items = if local.is_empty() {
                BTreeMap::new()
            } else {
                let runtime = self.local.context().await?;
                tokio::task::spawn_blocking(move || runtime.library.playlist_local_items(&local))
                    .await
                    .map_err(|_| Error::State("local playlist worker stopped"))??
            };
            let mut refs = BTreeMap::new();
            let slots = slice
                .into_iter()
                .map(|row| match row {
                    PlaylistLocalRow::Remote { index } => tracks
                        .get(*index)
                        .map(PlaylistPageSlot::Service)
                        .ok_or(Error::QueueChanged),
                    PlaylistLocalRow::Local(reference) => {
                        refs.insert(reference.id.clone(), reference.revision.clone());
                        let row = match items.remove(&reference.id) {
                            Some(item) => view::track(item.id, &item.metadata, false),
                            None => missing_playlist_row(reference.id.clone(), true),
                        };
                        Ok(PlaylistPageSlot::Local(Box::new(row)))
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            let rows = self
                .session
                .run_for(token, |current, _| async {
                    self.session.require_api(token, &api)?;
                    playlist_rows(current, slots, page.offset, trial_mode, None).await
                })
                .await?;
            // A concurrent local edit during song-detail lookup invalidates this page.
            if store.generation(token, id)? != generation {
                return Err(Error::QueueChanged);
            }
            let total = membership.rows.len();
            if total != tracks.len() {
                playlist.track_count = total;
            }
            let result = TrackPage {
                revision: Some(revision),
                total,
                more: page.offset.saturating_add(rows.len()) < total,
                offset: page.offset,
                tracks: rows,
                playlist: Some(playlist),
                playlist_local: Some(PlaylistLocalPage {
                    revision: membership.revision,
                    has_members: total != tracks.len(),
                    has_unbound_legacy: tracks.iter().any(|row| row["v"].as_i64() == Some(-1)),
                    rows: refs,
                }),
                playlist_offline: None,
                daily: None,
                style_tags: None,
            };
            // A failed cache write must not hide an otherwise valid live read.
            self.session.require_api(token, &api)?;
            let _ = self.observe_offline_playlist(
                token,
                &result,
                generation,
                detail["updateTime"].as_u64(),
            );
            OnlinePlaylistPage {
                page: result,
                api,
                membership_generation: generation,
            }
        };
        Ok(result)
    }
}
