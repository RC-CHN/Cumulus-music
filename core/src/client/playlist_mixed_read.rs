//! Assemble the owned playlist using the service order and the device overlay.
use super::{
    MusicClient, mapping, missing_playlist_row, playlist_rows, playlist_track_page,
    playlist_track_revision,
};
use crate::{
    Error, Result,
    client::{dto::*, local::view},
    playlist::{PlaylistApi, PlaylistLocalRow, PlaylistLocalStore},
    protocol::api::identifier,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) enum PlaylistPageSlot<'a> {
    Service(&'a Value),
    Local(TrackView),
}

impl MusicClient {
    pub async fn playlist_page_at(
        &self,
        id: &str,
        page: PageRequest,
        local_revision: Option<String>,
    ) -> Result<AccountData<TrackPage>> {
        mapping::page(page.offset, page.limit)?;
        let id = identifier(&json!(id))?;
        let token = self.session.token()?;
        let (api, uid, detail) = self
            .session
            .run_for(&token, |api, uid| async {
                let detail = PlaylistApi::new(api.clone()).detail(&json!(id)).await?;
                Ok((api, uid, detail))
            })
            .await?;
        let mut playlist = mapping::playlist(&detail, &uid)?;
        if playlist.id != id {
            return Err(Error::Response("playlist identity mismatch"));
        }
        let result = if !playlist.owned || playlist.favorite {
            if local_revision.is_some() {
                return Err(Error::QueueChanged);
            }
            playlist_track_page(api, &uid, &id, &detail, &page, None, None).await?
        } else {
            let revision = playlist_track_revision(&id, &detail)?;
            if page.revision.as_ref().is_some_and(|r| r != &revision) {
                return Err(Error::QueueChanged);
            }
            let store = PlaylistLocalStore::new(self.session.clone())?;
            let membership = store.project(&token, &id, &detail)?;
            if local_revision
                .as_ref()
                .is_some_and(|r| r != &membership.revision)
            {
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
                        Ok(PlaylistPageSlot::Local(match items.remove(&reference.id) {
                            Some(item) => view::track(item.id, &item.metadata, false),
                            None => missing_playlist_row(reference.id.clone(), true),
                        }))
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            let rows = playlist_rows(api, slots, page.offset, None, None).await?;
            // A concurrent local edit during song-detail lookup invalidates this page.
            if store.project(&token, &id, &detail)?.revision != membership.revision {
                return Err(Error::QueueChanged);
            }
            let total = membership.rows.len();
            if total != tracks.len() {
                playlist.track_count = total;
            }
            TrackPage {
                revision: Some(revision),
                total,
                more: page.offset.saturating_add(rows.len()) < total,
                offset: page.offset,
                tracks: rows,
                playlist: Some(playlist),
                playlist_local: Some(PlaylistLocalPage {
                    revision: membership.revision,
                    has_members: total != tracks.len(),
                    rows: refs,
                }),
                daily: None,
                style_tags: None,
            }
        };
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: token.generation.to_string(),
                value: result,
            })
        })
    }
}
