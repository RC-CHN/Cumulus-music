//! Song menu nextPlay: insert after the current entry without replacing playback.
//! Official playingList.addItemToCurPlayingList deduplicates by resource identity.
use super::*;
use crate::{
    catalog::CatalogApi,
    membership::Quality,
    playback::{
        QueueEntry,
        controller::{QueueTransport, QueueUpdate},
    },
    protocol::api::identifier,
};
use serde_json::json;
use std::sync::atomic::Ordering;

impl MusicClient {
    pub(super) async fn next_play(
        &self,
        epoch: &str,
        id: String,
        quality: Quality,
        expected_revision: String,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        self.session.with_token(&token, |_| Ok(()))?;
        let id = identifier(&json!(id))?;
        let controller = self.player.controller().await?;
        let (before, mut queue) = controller.queue_with_state(&token).await?;
        if expected_revision != before.queue_revision.to_string() {
            return Err(Error::QueueChanged);
        }
        if before.fm {
            return Err(Error::State("FM queue requires its coordinator"));
        }
        // Selecting the current song does not duplicate or restart it.
        if before
            .selected
            .as_ref()
            .is_some_and(|e| e.resource["id"] == id)
        {
            return project(&before, &token);
        }
        let focus = self.focus.snapshot()?;
        let attempt = self.player.selection.fetch_add(1, Ordering::AcqRel) + 1;
        let batch = self
            .session
            .run_for(&token, |api, _| {
                let id = id.clone();
                async move { CatalogApi::new(api).songs(&[json!(id)], 100).await }
            })
            .await?;
        let song = batch
            .songs
            .into_iter()
            .next()
            .flatten()
            .ok_or(Error::State("selected song is unavailable"))?;
        let view = mapping::track(&song)?;
        let resource = json!({"id":id,"level":quality.as_str(),"dt":view.duration_ms,
            "song":{"id":view.id,"name":view.name,"dt":view.duration_ms,"fee":view.fee,
            "ar":view.artist_refs.iter().map(|a|json!({"id":a.id,"name":a.name})).collect::<Vec<_>>(),
            "al":{"id":view.album_id,"name":view.album,"picUrl":view.cover_url}}});
        let existing = queue
            .items
            .iter()
            .find(|e| e.resource["id"] == id && e.resource.get("local_id").is_none())
            .cloned();
        let entry = existing.unwrap_or_else(|| QueueEntry {
            entry_id: uuid::Uuid::new_v4().to_string(),
            resource,
            source: json!({"kind":"songs"}),
        });
        let removed: Vec<_> = queue
            .items
            .iter()
            .filter(|e| e.resource["id"] == id && e.resource.get("local_id").is_none())
            .map(|e| e.entry_id.clone())
            .collect();
        queue.items.retain(|e| !removed.contains(&e.entry_id));
        queue.order.retain(|id| !removed.contains(id));
        let display = queue
            .items
            .iter()
            .position(|e| Some(&e.entry_id) == queue.current.as_ref())
            .map_or(0, |i| i + 1);
        let order = queue
            .order
            .iter()
            .position(|id| Some(id) == queue.current.as_ref())
            .map_or(0, |i| i + 1);
        queue.order.insert(order, entry.entry_id.clone());
        let start = queue.current.is_none();
        if start {
            queue.current = Some(entry.entry_id.clone());
        }
        queue.items.insert(display, entry);
        self.session.check_generation(&token)?;
        if self.player.selection.load(Ordering::Acquire) != attempt {
            return Err(Error::StaleResponse);
        }
        if start {
            self.claim_native_media(&token, super::super::focus::Owner::Audio, focus)
                .await?;
            if self.player.selection.load(Ordering::Acquire) != attempt {
                return Err(Error::StaleResponse);
            }
        }
        let value = controller
            .command(
                &token,
                PlaybackCommand::UpdateQueue(QueueUpdate {
                    expected: before.fence(),
                    queue,
                    transport: start.then_some(QueueTransport {
                        position: None,
                        paused: false,
                    }),
                }),
            )
            .await?;
        self.session.check_generation(&token)?;
        self.together_signal(&token, None);
        project(&value, &token)
    }
}
