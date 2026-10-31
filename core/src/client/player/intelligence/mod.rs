mod filter;
mod finish;
mod flight;
mod prepare;
use super::*;
use crate::{
    catalog::intelligence::{IntelligenceRequest, IntelligenceStart},
    playback::{
        PlaybackMode, QueueEntry, QueueSnapshot,
        controller::IntelligenceUpdate,
        intelligence::{IntelligenceAction, IntelligenceBegin, IntelligencePhase},
    },
    protocol::api::identifier,
};
use flight::Flight;
pub(super) use flight::IntelligenceHost;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum IntelligenceOrigin {
    Current,
    Likes {
        playlist_id: String,
        revision: Option<String>,
        first_id: Option<String>,
    },
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum IntelligenceCommand {
    Start {
        operation_id: String,
        expected_queue_revision: String,
        origin: IntelligenceOrigin,
    },
    Continue {
        operation_id: String,
        expected_queue_revision: String,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceEligibility {
    pub epoch: String,
    pub revision: String,
    pub can_enter: bool,
    pub liked_playlist_id: Option<String>,
}
impl MusicClient {
    pub fn player_intelligence_cancel(&self, epoch: &str, id: &str) -> Result<bool> {
        self.player
            .intelligence
            .cancel(&self.player_token(epoch)?, id)
    }
    fn intelligence_room(&self, token: &SessionToken) -> Result<()> {
        let room = self.queue_clear_context(token)?;
        if room.room_id.is_some() || room.issue.is_some() {
            return Err(Error::State("leave together before intelligence"));
        }
        Ok(())
    }
    pub async fn player_intelligence_eligibility(
        &self,
        epoch: &str,
        revision: &str,
    ) -> Result<IntelligenceEligibility> {
        let token = self.playlist_token(epoch)?;
        let controller = self.player.controller().await?;
        let before = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        if before.queue_revision.to_string() != revision {
            return Err(Error::QueueChanged);
        }
        let liked = self.intelligence_liked_id(&token, None).await?;
        let after = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        self.session.check_generation(&token)?;
        if after.queue_revision != before.queue_revision {
            return Err(Error::QueueChanged);
        }
        let can_enter = !before.fm
            && self.intelligence_room(&token).is_ok()
            && liked.as_ref().is_some_and(|id| {
                before.selected.as_ref().is_some_and(|e| {
                    e.source["kind"] == "playlist" && e.source["id"].as_str() == Some(id)
                })
            });
        Ok(IntelligenceEligibility {
            epoch: epoch.into(),
            revision: revision.into(),
            can_enter,
            liked_playlist_id: liked,
        })
    }
    pub async fn player_intelligence(
        &self,
        epoch: &str,
        command: IntelligenceCommand,
    ) -> Result<PlayerView> {
        let token = self.playlist_token(epoch)?;
        let (id, revision) = match &command {
            IntelligenceCommand::Start {
                operation_id,
                expected_queue_revision,
                ..
            }
            | IntelligenceCommand::Continue {
                operation_id,
                expected_queue_revision,
            } => (operation_id, expected_queue_revision),
        };
        let _gate = self
            .player
            .intelligence
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        let flight = self.player.intelligence.open(token.clone(), id.clone())?;
        let controller = self.player.controller().await?;
        let before = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        if before.queue_revision.to_string() != *revision {
            return Err(Error::QueueChanged);
        }
        flight.watch_queue(controller.subscribe(), before.queue_revision)?;
        self.intelligence_room(&token)?;
        match command {
            IntelligenceCommand::Continue { .. } => self.intelligence_finish(&flight).await,
            IntelligenceCommand::Start { origin, .. } => {
                if before.intelligence.as_ref().is_some_and(|s| s.active()) {
                    return Err(Error::State("intelligence mode is already active"));
                }
                let focus = self.focus.snapshot()?;
                let intent = self.player.selection.fetch_add(1, Ordering::AcqRel) + 1;
                let preserve = matches!(origin, IntelligenceOrigin::Current);
                let (playlist, revision, seed) = match origin {
                    IntelligenceOrigin::Current => {
                        let item = before
                            .selected
                            .as_ref()
                            .ok_or(Error::State("intelligence requires a current song"))?;
                        if item.source["kind"] != "playlist" {
                            return Err(Error::State("intelligence requires the liked playlist"));
                        }
                        (
                            identifier(&item.source["id"])?,
                            None,
                            Some(identifier(&item.resource["id"])?),
                        )
                    }
                    IntelligenceOrigin::Likes {
                        playlist_id,
                        revision,
                        first_id,
                    } => (
                        identifier(&json!(playlist_id))?,
                        revision,
                        first_id.map(|id| identifier(&json!(id))).transpose()?,
                    ),
                };
                if self
                    .intelligence_liked_id(&token, Some(&flight))
                    .await?
                    .as_ref()
                    != Some(&playlist)
                {
                    return Err(Error::State("intelligence requires the liked playlist"));
                }
                let prepared = self
                    .intelligence_prepare(&flight, &playlist, revision.as_deref(), intent)
                    .await?;
                self.intelligence_start_guard(&flight, intent)?;
                let explicit_seed = seed.is_some();
                let seed = if preserve {
                    // M/308 uses the actual firstTrack even if that song was
                    // removed from likes after playback began. The source
                    // playlist is checked above; final source authorization
                    // still belongs to the native resolver.
                    self.intelligence_current_seed(
                        &flight,
                        &before.selected.as_ref().unwrap().resource,
                    )
                    .await?
                } else {
                    match seed {
                        Some(id) => prepared
                            .resources
                            .iter()
                            .find(|r| r["id"].as_str() == Some(&id))
                            .cloned()
                            .ok_or(Error::State("intelligence seed is unavailable"))?,
                        None => {
                            use rand::Rng;
                            prepared.resources
                                [rand::rng().random_range(0..prepared.resources.len())]
                            .clone()
                        }
                    }
                };
                let seed_id = identifier(&seed["id"])?;
                let _room = self
                    .together
                    .gate
                    .try_lock()
                    .map_err(|_| Error::MutationBusy)?;
                self.intelligence_room(&token)?;
                self.intelligence_start_guard(&flight, intent)?;
                let mut current = controller
                    .command(&token, PlaybackCommand::Snapshot)
                    .await?;
                if current.queue_revision != before.queue_revision {
                    return Err(Error::QueueChanged);
                }
                self.focus.check_snapshot(focus)?;
                if current.fm {
                    current = self.player.fm().await?.leave_for(&token, intent).await?;
                }
                let lease = self
                    .claim_native_media(&token, crate::client::focus::Owner::Audio, focus)
                    .await?;
                self.intelligence_start_guard(&flight, intent)?;
                self.focus
                    .check(lease, crate::client::focus::Owner::Audio)?;
                let items: Vec<_> = prepared
                    .resources
                    .iter()
                    .map(|resource| QueueEntry {
                        entry_id: uuid::Uuid::new_v4().simple().to_string(),
                        resource: resource.clone(),
                        source: json!({"kind":"playlist","id":playlist}),
                    })
                    .collect();
                let request = IntelligenceRequest {
                    playlist_id: playlist.clone(),
                    song_id: seed_id,
                    start_music_id: identifier(&prepared.resources[0]["id"])?,
                    count: prepared.count,
                    start: if preserve || explicit_seed {
                        IntelligenceStart::PlayOne
                    } else {
                        IntelligenceStart::PlayAll
                    },
                };
                let liked = QueueSnapshot {
                    order: items.iter().map(|e| e.entry_id.clone()).collect(),
                    current: items.first().map(|e| e.entry_id.clone()),
                    items,
                    mode: PlaybackMode::OrderLoop,
                    ..Default::default()
                };
                controller
                    .command_guarded(
                        &token,
                        PlaybackCommand::Intelligence(IntelligenceUpdate {
                            expected: current.fence(),
                            action: IntelligenceAction::Begin(Box::new(IntelligenceBegin {
                                operation_id: Some(flight.operation_id().into()),
                                request,
                                seed_resource: seed,
                                seed_source: json!({"kind":"playlist","id":playlist}),
                                liked,
                                preserve_current_queue: preserve,
                                liked_metadata: prepared.metadata,
                            })),
                        }),
                        current.fence(),
                        flight.native_cancel(),
                    )
                    .await?;
                drop(_room);
                self.intelligence_finish(&flight).await
            }
        }
    }
    fn intelligence_start_guard(&self, flight: &Flight, intent: u64) -> Result<()> {
        flight.check(&self.session)?;
        if self.player.selection.load(Ordering::Acquire) != intent {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}
