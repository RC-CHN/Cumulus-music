mod prepare;
mod view;
use super::{MusicClient, dto::AccountData, focus::Owner};
use crate::{
    Error, Result,
    membership::Quality,
    playback::{
        PlaybackMode, QueueEntry, QueueSnapshot,
        controller::{PlaybackCommand, QueueTransport, QueueUpdate},
    },
    relay::{
        PullPhase, RelayPlayback, RelayPlaybackPhase, RelayPlaybackRequest, RelayService,
        validate_id,
    },
};
use rand::seq::SliceRandom;
use serde::Deserialize;
use serde_json::json;
use std::sync::atomic::Ordering;
pub use view::*;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RelayCommand {
    Continue {
        id: String,
        revision: String,
        quality: Quality,
    },
    Retry {
        id: String,
        previous_id: String,
        quality: Quality,
    },
    Dismiss {
        revision: String,
    },
    CancelUnseen {
        id: String,
        revision: String,
    },
}
impl MusicClient {
    pub async fn relay_command(
        &self,
        epoch: &str,
        command: RelayCommand,
    ) -> Result<AccountData<RelayView>> {
        let token = self.player_token(epoch)?;
        let applications = RelayPlayback::new(self.session.clone())?;
        let (owner, _lock) = applications.lock(&token)?;
        applications.recover(&token)?;
        let service = RelayService::new(self.session.clone())?;
        let (id, pull_id, previous, quality, revision) = match command {
            RelayCommand::Dismiss { revision } => {
                service.dismiss_for(&token, &revision)?;
                return self.relay_view(epoch, false).await;
            }
            RelayCommand::CancelUnseen { id, revision } => {
                applications.cancel_unseen(&token, &id)?;
                service.cancel_unseen_for(&token, &id, &revision)?;
                return self
                    .relay_view_with_operation(epoch, false, Some(&id))
                    .await;
            }
            RelayCommand::Continue {
                id,
                revision,
                quality,
            } => {
                validate_id(&id)?;
                if revision.len() != 64 || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(Error::InvalidInput("invalid relay revision"));
                }
                (id.clone(), id, None, quality, Some(revision))
            }
            RelayCommand::Retry {
                id,
                previous_id,
                quality,
            } => {
                let previous = applications
                    .record(&token, &previous_id)?
                    .ok_or(Error::State("relay retry intent missing"))?;
                (id, previous.pull_id, Some(previous_id), quality, None)
            }
        };
        if self.has_together_cleanup()? {
            return Err(Error::State("leave together before relay continuation"));
        }
        let Some(mut record) = applications.begin(
            &token,
            &id,
            &pull_id,
            previous.as_deref(),
            RelayPlaybackRequest {
                revision: revision.clone(),
                quality,
            },
        )?
        else {
            return self
                .relay_view_with_operation(epoch, false, Some(&id))
                .await;
        };
        let result = async {
            let lease = self.claim_media(&token, Owner::Audio).await?;
            let attempt = self.player.selection.fetch_add(1, Ordering::AcqRel) + 1;
            let controller = self.player.controller().await?;
            let (before, _) = controller.queue_with_state(&token).await?;
            let intent = if let Some(revision) = revision {
                service.pull_for(&token, &pull_id, &revision).await?
            } else {
                service
                    .intent_for(&token, &pull_id)?
                    .ok_or(Error::State("relay pull receipt missing"))?
            };
            if intent.phase != PullPhase::Ready {
                return Err(Error::State("relay pull has no confirmed playback plan"));
            }
            let plan = intent
                .plan
                .ok_or(Error::State("relay playback plan missing"))?;
            let mut prepared = self.prepare_relay(&token, &plan, quality, attempt).await?;
            if self.player.selection.load(Ordering::Acquire) != attempt {
                return Err(Error::StaleResponse);
            }
            self.focus.check(lease, Owner::Audio)?;
            self.session.check_generation(&token)?;
            prepared.source["relay_application"] = json!(id);
            let items = prepared
                .resources
                .into_iter()
                .map(|resource| QueueEntry {
                    entry_id: uuid::Uuid::new_v4().simple().to_string(),
                    resource,
                    source: prepared.source.clone(),
                })
                .collect::<Vec<_>>();
            let current = Some(items[prepared.index].entry_id.clone());
            let mode = plan.mode.map(Into::into).unwrap_or(before.mode);
            let mut order = items.iter().map(|e| e.entry_id.clone()).collect::<Vec<_>>();
            if mode == PlaybackMode::Random {
                order.shuffle(&mut rand::rng());
            }
            record.phase = RelayPlaybackPhase::Applying;
            self.session
                .with_token(&token, |owner| applications.save_original(owner, &record))?;
            controller
                .command(
                    &token,
                    PlaybackCommand::UpdateQueue(QueueUpdate {
                        expected: before.fence(),
                        queue: QueueSnapshot {
                            items,
                            order,
                            current,
                            mode,
                        },
                        transport: Some(QueueTransport {
                            position: Some(plan.position_ms as f64 / 1000.0),
                            paused: false,
                        }),
                    }),
                )
                .await?;
            self.session.check_generation(&token)?;
            Ok(())
        }
        .await;
        record.phase = match result {
            Ok(()) => RelayPlaybackPhase::Installed,
            Err(_) if record.phase == RelayPlaybackPhase::Applying => RelayPlaybackPhase::Uncertain,
            Err(_) => RelayPlaybackPhase::Failed,
        };
        applications.save_original(&owner, &record)?;
        self.session.check_generation(&token)?;
        // The typed page exposes durable outcome instead of private network
        // errors or source URLs. Player load failures remain player state.
        self.relay_view_with_operation(epoch, false, Some(&id))
            .await
    }
}
