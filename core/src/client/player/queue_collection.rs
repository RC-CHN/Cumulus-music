//! Official Playlist.tsx collection eligibility, with native local-file mapping.
use super::*;
use crate::{
    library::{LocalMapping, LocalResource},
    protocol::api::identifier,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

impl MusicClient {
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
