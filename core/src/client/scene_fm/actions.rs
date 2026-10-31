use super::*;
use crate::{
    client::{MusicClient, dto::AccountData, focus::Owner},
    playback::{
        PlaybackMode, QueueEntry, QueueSnapshot,
        controller::{PlaybackCommand, PlaybackStatus, QueueTransport, QueueUpdate},
    },
    podcast::scene::SceneFmService,
};
use rand::seq::SliceRandom;
use serde_json::json;
use std::sync::atomic::Ordering;
impl MusicClient {
    pub async fn scene_fm_command(
        &self,
        epoch: &str,
        command: SceneFmCommand,
    ) -> Result<AccountData<SceneFmView>> {
        let token = self.player_token(epoch)?;
        self.session.with_token(&token, |_| Ok(()))?;
        // Concurrent page visits must not run the once-only auto-start twice.
        let _operation = self
            .scene_fm
            .operation
            .try_lock()
            .map_err(|_| Error::State("scene FM operation in progress"))?;
        let service = SceneFmService::new(self.session.clone())?;
        let rev = match &command {
            SceneFmCommand::Autoplay { enabled } => {
                service.set_autoplay_for(&token, *enabled)?;
                return self.scene_fm_view(epoch, false).await;
            }
            SceneFmCommand::Cancel { revision: r } => {
                service.cancel_for(&token, revision(r)?)?;
                return self.scene_fm_view(epoch, false).await;
            }
            SceneFmCommand::Select { revision, .. }
            | SceneFmCommand::Play { revision }
            | SceneFmCommand::Toggle { revision }
            | SceneFmCommand::Automatic { revision } => revision.clone(),
        };
        let rev = revision(&rev)?;
        let mut desired = service
            .cached_for(&token)?
            .ok_or(Error::State("scene FM not loaded"))?;
        if desired.revision != rev {
            return Err(Error::QueueChanged);
        }
        let automatic = matches!(command, SceneFmCommand::Automatic { .. });
        if automatic
            && (!service.autoplay_for(&token)? || desired.pending || self.has_together_cleanup()?)
        {
            return self.scene_fm_view(epoch, false).await;
        }
        if self.has_together_cleanup()? {
            return Err(Error::State("leave together before entering scene FM"));
        }
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(&token).await?;
        let generation = if automatic {
            if matches!(
                before.state,
                PlaybackStatus::Playing
                    | PlaybackStatus::Buffering
                    | PlaybackStatus::Loading
                    | PlaybackStatus::Preparing
            ) {
                return self.scene_fm_view(epoch, false).await;
            }
            let Some(g) = self.focus.automatic_audio_generation()? else {
                return self.scene_fm_view(epoch, false).await;
            };
            Some(g)
        } else {
            None
        };
        if !self.scene_fm.attempt(&token, automatic)? {
            return self.scene_fm_view(epoch, false).await;
        }
        let intent = if automatic {
            self.player.selection.load(Ordering::Acquire)
        } else {
            self.player.selection.fetch_add(1, Ordering::AcqRel) + 1
        };
        let manual_lease = if automatic {
            None
        } else {
            Some(self.claim_media(&token, Owner::Audio).await?)
        };
        // The same cover button controls only its own current voice. A newer
        // stop/selection is rejected atomically by the playback fence.
        let same = before.selected.as_ref().is_some_and(|e| {
            e.source["kind"] == "scene_fm"
                && e.source["scene_id"].as_str() == desired.active_id.as_deref()
                && desired
                    .selected()
                    .is_some_and(|v| e.resource["voice_id"] == v.id)
        });
        if matches!(command, SceneFmCommand::Toggle { .. })
            && same
            && !desired.pending
            && matches!(
                before.state,
                PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
            )
        {
            self.focus
                .check(manual_lease.expect("manual"), Owner::Audio)?;
            controller
                .command(
                    &token,
                    PlaybackCommand::UpdateQueue(QueueUpdate {
                        expected: before.fence(),
                        queue,
                        transport: Some(QueueTransport {
                            position: before.position,
                            paused: before.state != PlaybackStatus::Paused,
                        }),
                    }),
                )
                .await?;
            return self.scene_fm_view(epoch, false).await;
        }
        if let SceneFmCommand::Select { scene_id, .. } = &command {
            desired = service.select_for(&token, rev, scene_id).await?;
        }
        let (prepared, resources, index) = service.prepare_for(&token, desired.revision).await?;
        let apply = async {
            self.session.check_generation(&token)?;
            if automatic {
                self.player
                    .selection
                    .compare_exchange(intent, intent + 1, Ordering::AcqRel, Ordering::Acquire)
                    .map_err(|_| Error::StaleResponse)?;
            } else if self.player.selection.load(Ordering::Acquire) != intent {
                return Err(Error::StaleResponse);
            }
            let lease = if let Some(lease) = manual_lease {
                lease
            } else {
                self.claim_media_if(&token, Owner::Audio, generation)
                    .await?
            };
            self.focus.check(lease, Owner::Audio)?;
            let source = json!({"kind":"scene_fm","scene_id":prepared.active_id,"scene_revision":prepared.revision});
            let items: Vec<_> = resources
                .into_iter()
                .map(|resource| QueueEntry {
                    entry_id: uuid::Uuid::new_v4().simple().to_string(),
                    resource,
                    source: source.clone(),
                })
                .collect();
            let current = Some(items[index].entry_id.clone());
            let mut order = items.iter().map(|e| e.entry_id.clone()).collect::<Vec<_>>();
            if before.mode == PlaybackMode::Random {
                order.shuffle(&mut rand::rng());
            }
            controller
                .command(
                    &token,
                    PlaybackCommand::UpdateQueue(QueueUpdate {
                        expected: before.fence(),
                        queue: QueueSnapshot {
                            items,
                            order,
                            current,
                            mode: before.mode,
                            fm_source: None,
                            intelligence: None,
                        },
                        transport: Some(QueueTransport {
                            position: None,
                            paused: false,
                        }),
                    }),
                )
                .await?;
            Ok(())
        }
        .await;
        if automatic
            && apply.as_ref().is_err_and(|e| {
                matches!(
                    e.root(),
                    Error::StaleResponse | Error::QueueChanged | Error::Cancelled
                )
            })
        {
            // A newer local intent wins. Do not leave an automatic retry armed.
            service.cancel_for(&token, prepared.revision)?;
        }
        apply?;
        self.scene_fm_view(epoch, false).await
    }
}
