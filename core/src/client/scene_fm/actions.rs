use super::*;
use crate::{
    client::{MusicClient, dto::AccountData, focus::Owner},
    playback::{
        PlaybackMode, QueueEntry, QueueSnapshot,
        controller::{PlaybackCommand, PlaybackStatus, QueueTransport, QueueUpdate},
        engine::Cancellation,
    },
    podcast::scene::SceneFmService,
};
use rand::seq::SliceRandom;
use serde_json::json;
use std::sync::{Arc, atomic::Ordering};
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
        let service = Arc::new(SceneFmService::new(self.session.clone())?);
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
            | SceneFmCommand::HomeAutomatic { revision, .. } => revision.clone(),
        };
        let rev = revision(&rev)?;
        let mut desired = service
            .cached_for(&token)?
            .ok_or(Error::State("scene FM not loaded"))?;
        if desired.revision != rev {
            return Err(Error::QueueChanged);
        }
        let home = match &command {
            SceneFmCommand::HomeAutomatic { visit, .. } => {
                let visit = self.scene_fm.home_visit(&token, visit)?;
                visit.check(&self.session, &token)?;
                Some(visit)
            }
            _ => None,
        };
        let automatic = home.is_some();
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
        let initial = if let Some(visit) = home.as_ref() {
            visit.check(&self.session, &token)?;
            let Some(initial) = visit.audio.as_ref() else {
                return self.scene_fm_view(epoch, false).await;
            };
            if !home::idle_for_automatic(before.state)
                || before.fence() != initial.fence
                || self.player.selection.load(Ordering::Acquire) != initial.selection
                || self.focus.snapshot()? != initial.focus
            {
                return self.scene_fm_view(epoch, false).await;
            }
            Some(initial)
        } else {
            None
        };
        let intent = initial.map_or_else(
            || self.player.selection.fetch_add(1, Ordering::AcqRel) + 1,
            |initial| initial.selection,
        );
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
        // A current item's pause/resume is a transport action, not the
        // official playSceneFm path which consumes the module latch.
        if !self.scene_fm.attempt(&token, automatic)? {
            return self.scene_fm_view(epoch, false).await;
        }
        if let SceneFmCommand::Select { scene_id, .. } = &command {
            desired = service.select_for(&token, rev, scene_id).await?;
        }
        let (prepared, resources, index) = service.prepare_for(&token, desired.revision).await?;
        let apply = async {
            self.session.check_generation(&token)?;
            let (lease, authority) = if let (Some(visit), Some(initial)) = (home.as_ref(), initial) {
                visit.check(&self.session, &token)?;
                if self.login_pending()?
                    || !service.autoplay_for(&token)?
                    || self.has_together_cleanup()?
                    || controller.snapshot().fence() != initial.fence
                {
                    return Err(Error::StaleResponse);
                }
                let updated = intent
                    .checked_add(1)
                    .ok_or(Error::State("playback selection exhausted"))?;
                let (lease, ()) = self.reserve_prepared_audio(&token, initial.focus, || {
                    visit.check(&self.session, &token)?;
                    self.player
                        .selection
                        .compare_exchange(intent, updated, Ordering::AcqRel, Ordering::Acquire)
                        .map(|_| ())
                        .map_err(|_| Error::StaleResponse)
                })?;
                let lease = self.finish_media_claim(&token, Owner::Audio, lease).await?;
                let (session, scope, selection, preference, owner, focus) = (
                    self.session.clone(),
                    visit.clone(),
                    self.player.selection.clone(),
                    service.clone(),
                    token.clone(),
                    self.focus.clone(),
                );
                let together = self.together_detached_guard(&token);
                let authority = Cancellation::new(move || {
                    selection.load(Ordering::Acquire) == updated
                        && focus.check(lease, Owner::Audio).is_ok()
                        && !together.is_cancelled()
                        && scope.check(&session, &owner).is_ok()
                        && preference.autoplay_for(&owner).is_ok_and(|enabled| enabled)
                });
                (lease, Some(authority))
            } else {
                if self.player.selection.load(Ordering::Acquire) != intent {
                    return Err(Error::StaleResponse);
                }
                (manual_lease.expect("manual"), None)
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
            let fence = before.fence();
            let command = PlaybackCommand::UpdateQueue(QueueUpdate {
                expected: fence.clone(),
                queue: QueueSnapshot {
                    items,
                    order,
                    current,
                    mode: before.mode,
                    fm_source: None,
                    intelligence: None,
                    more: None,
                },
                transport: Some(QueueTransport {
                    position: None,
                    paused: false,
                }),
            });
            if let Some(authority) = authority {
                controller
                    .command_guarded(&token, command, fence, authority)
                    .await?;
            } else {
                controller.command(&token, command).await?;
            }
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
