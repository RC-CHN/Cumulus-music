//! Official Playlist.tsx collection eligibility, with native local-file mapping.
use super::*;
use crate::{
    library::{LocalMapping, LocalResource},
    protocol::api::identifier,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

impl MusicClient {
    /// Freeze the inline action's ordered visible entry selection. This lane
    /// never falls back to collecting the queue or mapping a foreign resource.
    pub async fn player_queue_collect_rows(
        &self,
        epoch: &str,
        revision: &str,
        entry_ids: &[String],
    ) -> Result<QueueCollectionView> {
        let token = self.playlist_token(epoch)?;
        let revision = revision
            .parse::<u64>()
            .map_err(|_| Error::InvalidInput("invalid collection queue revision"))?;
        if entry_ids.is_empty() || entry_ids.len() > queue_playlist::QUEUE_LIMIT {
            return Err(Error::InvalidInput("invalid queue collection selection"));
        }
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(&token).await?;
        if before.queue_revision != revision {
            return Err(Error::QueueChanged);
        }
        if before.fm || self.queue_clear_context(&token)?.room_id.is_some() {
            return Err(Error::InvalidInput(
                "song collection requires the ordinary queue",
            ));
        }
        let visible: BTreeMap<_, _> = queue
            .items
            .iter()
            .filter(|entry| entry.visible_in_queue())
            .map(|entry| (entry.entry_id.as_str(), &entry.resource))
            .collect();
        let mut entries = BTreeSet::new();
        let mut songs = BTreeSet::new();
        let mut tracks = Vec::new();
        let mut duplicates = 0;
        for id in entry_ids {
            if !entries.insert(id.as_str()) {
                return Err(Error::InvalidInput("duplicate queue entry in collection"));
            }
            let resource = visible.get(id.as_str()).ok_or(Error::QueueChanged)?;
            let song_id = row_namespace(resource)
                .then(|| queue_like::song_id(resource))
                .flatten()
                .ok_or(Error::InvalidInput(
                    "selection contains a row without a catalog song",
                ))?;
            if songs.insert(song_id.clone()) {
                let mut view = track(resource)?;
                view.id = song_id;
                tracks.push(view);
            } else {
                duplicates += 1;
            }
        }
        let after = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        self.session.with_token(&token, |_| Ok(()))?;
        if after.queue_revision != revision
            || after.fm
            || self.queue_clear_context(&token)?.room_id.is_some()
        {
            return Err(Error::QueueChanged);
        }
        Ok(QueueCollectionView {
            epoch: token.generation.to_string(),
            revision: revision.to_string(),
            total: entry_ids.len(),
            unsupported: 0,
            duplicates,
            tracks,
        })
    }

    /// Capture the displayed songs without HTTP. The destination selection uses
    /// the existing journaled playlist flow after this readonly preparation.
    pub async fn player_queue_collection(
        &self,
        epoch: &str,
        revision: &str,
    ) -> Result<QueueCollectionView> {
        let token = self.playlist_token(epoch)?;
        let revision = revision
            .parse::<u64>()
            .map_err(|_| Error::InvalidInput("invalid collection queue revision"))?;
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(&token).await?;
        if before.queue_revision != revision {
            return Err(Error::QueueChanged);
        }
        let session = self.session.clone();
        let captured = token.clone();
        let store = self.player.store.clone();
        let value = tokio::task::spawn_blocking(move || {
            let local_ids = queue
                .items
                .iter()
                .filter_map(|item| item.resource["local_id"].as_str().map(str::to_owned))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let mappings = if local_ids.is_empty() {
                BTreeMap::new()
            } else {
                LocalLibrary::new(store)?
                    .online_mappings_for_collection(&session, &captured, &local_ids)?
            };
            let mut tracks = Vec::new();
            let mut seen = BTreeSet::new();
            let mut unsupported = 0;
            let mut duplicates = 0;
            let mut total = 0;
            for item in &queue.items {
                if !item.visible_in_queue() {
                    continue;
                }
                total += 1;
                let Some(track) = collection_track(&item.resource, &mappings)? else {
                    unsupported += 1;
                    continue;
                };
                if seen.insert(track.id.clone()) {
                    tracks.push(track);
                } else {
                    duplicates += 1;
                }
            }
            session.with_token(&captured, |_| Ok(()))?;
            Ok::<_, Error>(QueueCollectionView {
                epoch: captured.generation.to_string(),
                revision: revision.to_string(),
                total,
                unsupported,
                duplicates,
                tracks,
            })
        })
        .await
        .map_err(|_| Error::State("queue collection worker stopped"))??;
        let after = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        self.session.with_token(&token, |_| Ok(()))?;
        if after.queue_revision != revision {
            return Err(Error::QueueChanged);
        }
        Ok(value)
    }
}

pub(super) fn row_namespace(resource: &Value) -> bool {
    identity::kind(resource) == PlayerResourceKind::Catalog
        && resource["voice"].is_null()
        && resource["download_voice_id"].is_null()
}

fn collection_track(
    resource: &Value,
    mappings: &BTreeMap<String, LocalMapping>,
) -> Result<Option<super::super::dto::TrackView>> {
    // Episode playback IDs authorize audio; they are not catalogue-song IDs.
    if resource["voice"].is_object()
        || !resource["voice_id"].is_null()
        || !resource["song"]["voice_id"].is_null()
    {
        return Ok(None);
    }
    if let Some(id) = resource["local_id"].as_str() {
        let Some(mapping) = mappings.get(id) else {
            return Ok(None);
        };
        let local: LocalResource = serde_json::from_value(resource.clone())?;
        if local.local_revision != mapping.revision {
            return Ok(None);
        }
        let id = identifier(&serde_json::json!(mapping.song_id))?;
        let mut view = match mapping.song.as_ref() {
            Some(song) => mapping::track(song)?,
            None => track(resource)?,
        };
        view.id = id;
        view.local = false;
        view.voice_id = None;
        Ok(Some(view))
    } else {
        let Ok(id) = identifier(&resource["id"]) else {
            return Ok(None);
        };
        let mut view = track(resource)?;
        view.id = id;
        Ok(Some(view))
    }
}
