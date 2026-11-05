//! M/3085 and M/1384 keep the existing queue for both batch buttons.
use super::{
    MusicClient, PlayerView, project,
    queue_playlist::{catalog_queue_resource, insert_zone_batch},
};
use crate::{
    Error, Result,
    catalog::CatalogApi,
    client::focus::Owner,
    membership::Quality,
    playback::{
        controller::{PlaybackCommand, QueueTransport, QueueUpdate},
        engine::Cancellation,
    },
    protocol::api::identifier,
};
use serde_json::json;
use std::sync::atomic::Ordering;

pub(super) struct ZoneBatchSelection {
    pub id: String,
    pub receipt: String,
    pub ids: Vec<String>,
    pub revision: String,
    pub quality: Quality,
}

impl MusicClient {
    pub(super) async fn zone_playlist_batch_action(
        &self,
        epoch: &str,
        input: ZoneBatchSelection,
        expected_queue_revision: Option<String>,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        let id = identifier(&json!(input.id))?;
        let context = self
            .zone_playlist_batch_context(&token, &input.receipt, &id, &input.revision)
            .await?;
        let ids = super::playlist_batch::selected_ids(
            &id,
            &context.snapshot.detail,
            &input.ids,
            &input.revision,
        )?;
        if context.guard.is_cancelled() {
            return Err(Error::StaleResponse);
        }
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(&token).await?;
        let play = expected_queue_revision.is_none();
        if expected_queue_revision
            .is_some_and(|expected| before.queue_revision.to_string() != expected)
        {
            return Err(Error::QueueChanged);
        }
        if before.fm || queue.intelligence.is_some() || self.together_attached(&token, None)? {
            return Err(Error::State(
                "zone playlist batch requires ordinary playback",
            ));
        }
        let focus = self.focus.snapshot()?;
        let attempt = self.player.selection.fetch_add(1, Ordering::AcqRel) + 1;
        let trial = context.snapshot.trial_mode;
        let mut resources = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(100) {
            if context.guard.is_cancelled()
                || self.player.selection.load(Ordering::Acquire) != attempt
            {
                return Err(Error::StaleResponse);
            }
            let songs = self
                .session
                .run_for(&token, |api, _| async move {
                    CatalogApi::new(api)
                        .songs_with_trial(chunk, 100, Some(trial))
                        .await
                })
                .await?;
            // Do not commit only the prefix or filter a missing selected
            // song into a different playback request. The real source still
            // needs the native URL authorization when a track is selected.
            if !songs.missing.is_empty() {
                return Err(Error::State("selected zone playlist songs are unavailable"));
            }
            for song in songs.songs {
                let mut resource = catalog_queue_resource(
                    &song.ok_or(Error::State("selected zone playlist songs are unavailable"))?,
                    input.quality,
                )?;
                resource["trial_mode"] = json!(trial);
                resources.push(resource);
            }
        }
        let was_selected = queue.selected();
        let start = play || was_selected.is_none();
        let detail = &context.snapshot.detail;
        let source = json!({
            "kind":"playlist","id":id,"trial_mode":trial,
            "history_resource":{"id":id,"name":detail["name"],"coverImgUrl":detail["coverImgUrl"],
                "creator":detail["creator"],"description":detail["description"],"trackCount":detail["trackCount"]}
        });
        let queue = insert_zone_batch(queue, resources, source, play)?;
        if context.guard.is_cancelled() || self.player.selection.load(Ordering::Acquire) != attempt
        {
            return Err(Error::StaleResponse);
        }
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        if self.together_attached(&token, None)? {
            return Err(Error::State(
                "zone playlist batch requires ordinary playback",
            ));
        }
        let lease = if start {
            Some(self.claim_native_media(&token, Owner::Audio, focus).await?)
        } else {
            None
        };
        let expected = before.fence();
        let position = (was_selected != queue.selected()).then_some(0.0);
        let command = PlaybackCommand::UpdateQueue(QueueUpdate {
            expected: expected.clone(),
            queue,
            transport: start.then_some(QueueTransport {
                position,
                paused: false,
            }),
        });
        let selection = self.player.selection.clone();
        let audio = self.focus.clone();
        let guard = context.guard;
        let value = controller
            .command_guarded(
                &token,
                command,
                expected,
                Cancellation::new(move || {
                    !guard.is_cancelled()
                        && selection.load(Ordering::Acquire) == attempt
                        && lease.is_none_or(|lease| audio.check(lease, Owner::Audio).is_ok())
                }),
            )
            .await?;
        self.session.check_generation(&token)?;
        project(&value, &token)
    }
}
