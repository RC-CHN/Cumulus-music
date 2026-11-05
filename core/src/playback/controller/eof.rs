//! M308 natural EOF, separately from manual transport, preload and decoder errors.
use super::{Message, PlaybackFence, PlaybackStatus, load::LoadOptions, state::Actor};
use crate::{
    Error, Result,
    account::SessionToken,
    playback::{
        PlaybackMode, QueueSnapshot, Step, TimelineMode,
        engine::Cancellation,
        preflight::OrdinaryQueueEnvironment,
        skip_policy::{self, OrdinarySkipDecision, OrdinarySkipTrigger},
    },
    protocol::{pc_experiments::PcExperimentGroups, wire::serialization::js_truthy},
};
use std::sync::Arc;
use tokio::task::JoinHandle;
use uuid::Uuid;

pub(crate) struct VerifiedOrdinarySkip {
    pub groups: PcExperimentGroups,
    pub authority: Cancellation,
}

pub(crate) type OrdinaryEofAuthority =
    Arc<dyn Fn(&SessionToken) -> Result<Option<VerifiedOrdinarySkip>> + Send + Sync>;

const TRIGGER: OrdinarySkipTrigger = OrdinarySkipTrigger::NativeEof;
const ENVIRONMENT: OrdinaryQueueEnvironment =
    OrdinaryQueueEnvironment::OnlineWithoutMvExperiment { app_online: true };

pub(super) struct Flight {
    id: Uuid,
    worker: JoinHandle<()>,
}

impl Drop for Flight {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

struct Ticket {
    token: SessionToken,
    generation: u64,
    native_entry: i64,
    fence: PlaybackFence,
    queue: QueueSnapshot,
    was_paused: bool,
    authority: Cancellation,
}

enum Outcome {
    KeepFinished,
    StopFinished,
    PlayExisting(String),
}

pub(super) struct Ready {
    id: Uuid,
    ticket: Ticket,
    result: Result<Outcome>,
}

impl Actor {
    /// Invoked only after a matching successful native EOF has been accepted
    /// and the old row's completion saved. A generic host has no admission.
    pub(super) fn schedule_ordinary_eof(&mut self, was_paused: bool) -> bool {
        let (Some(provider), Some(loaded)) = (&self.ordinary_eof, &self.loaded) else {
            return false;
        };
        if self.queue.mode() != PlaybackMode::Sequential
            || self.queue.len() > crate::playback::intelligence::MAX_QUEUE_TRACKS
            || self.play_id.is_none()
            || !self.ordinary_online_song(&loaded.item)
            || self.queue.selected().as_ref() != Some(&loaded.item)
            || loaded.source.info["kind"] != "online"
            || loaded.timeline.snapshot().mode != TimelineMode::Full
        {
            return false;
        }

        let Some(adjacent) = self.queue.peek(Step::Next, true) else {
            return false;
        };
        if !skip_policy::plain_copyright_denial(&adjacent) {
            return false;
        }
        let queue = self.queue.snapshot();
        let Ok(attempted) = skip_policy::initial_row(&queue, TRIGGER) else {
            return false;
        };
        if attempted != &adjacent
            || !queue.items.iter().map(|row| &row.entry_id).eq(&queue.order)
            || queue.items.iter().any(|row| {
                // No original replacement audio, video, or local handoff in
                // this first branch. M59 reads existing native evidence only.
                let backup = &row.resource["song"]["noCopyrightRcmd"];
                (!backup.is_null() && backup["type"].as_i64() != Some(0))
                    || !row.resource["native_open_file"].is_null()
                    || !row.resource["shared_room"].is_null()
                    || js_truthy(&row.resource["song"]["isOfflineDownOnline"])
                    || row.source["kind"] == "scene_fm"
            })
        {
            return false;
        }

        let verified = match provider(&self.token) {
            Ok(Some(value)) => value,
            Ok(None) => return false,
            // Old audio ownership (or another native observation error)
            // cannot authorize an automatic handoff on a different owner.
            Err(_) => return true,
        };
        let id = Uuid::new_v4();
        let attempted = attempted.clone();
        let ticket = Ticket {
            token: self.token.clone(),
            generation: self.generation(),
            native_entry: loaded.media.entry_id,
            fence: self.snapshot().fence(),
            queue,
            was_paused,
            authority: verified.authority,
        };
        let resolver = self.resolver.clone();
        let sender = self.sender.clone();
        let worker = tokio::spawn(async move {
            let result = async {
                ticket.authority.check()?;
                let denied = resolver
                    .confirmed_plain_skip_denial(&ticket.token, &attempted)
                    .await?;
                ticket.authority.check()?;
                if !denied {
                    return Ok(Outcome::PlayExisting(attempted.entry_id));
                }
                if !verified.groups.skip_songs_assigned() {
                    return Ok(Outcome::KeepFinished);
                }
                let scan = resolver
                    .ordinary_queue_preflight(&ticket.token, &ticket.queue, ENVIRONMENT)
                    .await?;
                ticket.authority.check()?;
                // A mixed online/download queue needs the original local
                // resolver sequence. Keep the known-denied row unselected.
                if scan.rows.iter().any(|row| !row.no_download_publication) {
                    return Ok(Outcome::KeepFinished);
                }
                Ok(
                    match skip_policy::after_initial_denial(&ticket.queue, &scan, TRIGGER)? {
                        OrdinarySkipDecision::StopAtDeniedTail { .. }
                        | OrdinarySkipDecision::StopAtErrorLimit { .. } => Outcome::StopFinished,
                        OrdinarySkipDecision::PlayExisting { target, .. } => {
                            Outcome::PlayExisting(target)
                        }
                        // No candidate / incomplete facts do not give this
                        // bounded branch authority to advance a finished source.
                        _ => Outcome::KeepFinished,
                    },
                )
            }
            .await;
            let _ = sender.send(Message::OrdinaryEof(Box::new(Ready { id, ticket, result })));
        });
        self.eof_flight = Some(Flight { id, worker });
        true
    }

    pub(super) async fn finish_ordinary_eof(&mut self, value: Ready) -> Result<()> {
        if self
            .eof_flight
            .as_ref()
            .is_none_or(|flight| flight.id != value.id)
        {
            return Ok(());
        }
        self.eof_flight = None;
        let Ticket {
            token,
            generation,
            native_entry,
            fence,
            queue,
            was_paused,
            authority,
        } = value.ticket;
        if self.state != PlaybackStatus::Ended
            || self.generation() != generation
            || self.token != token
            || authority.is_cancelled()
            || self.session.check_generation(&token).is_err()
            || self.loaded.as_ref().map(|loaded| loaded.media.entry_id) != Some(native_entry)
        {
            return Ok(());
        }
        self.queue.reload()?;
        if self.snapshot().fence() != fence
            || self.queue.snapshot() != queue
            || authority.is_cancelled()
        {
            return Ok(());
        }
        match value.result {
            Ok(Outcome::StopFinished) => {
                // Stop retires the actual decoder and play identity, but does
                // not clear the row or overwrite the saved completion.
                let position = self.position;
                self.stop(false).await?;
                self.error = None;
                self.queue.reload()?;
                if self.queue.snapshot() == queue {
                    self.position = position;
                }
            }
            Ok(Outcome::PlayExisting(target)) => {
                // Queue::select compares the durable queue under its write
                // transaction; hold the account generation through that commit.
                let selected = self
                    .session
                    .with_token(&token, |_| self.queue.select(&target));
                match selected {
                    Ok(_) => {}
                    Err(Error::QueueChanged | Error::StaleResponse) => return Ok(()),
                    Err(error) => return Err(error),
                }
                self.begin(
                    LoadOptions {
                        paused: was_paused,
                        ..Default::default()
                    },
                    Uuid::new_v4().simple().to_string(),
                    None,
                )
                .await?;
            }
            // Observation failures are not copyright or decoder failures.
            // A failed or revoked classification has no second transport.
            Ok(Outcome::KeepFinished) | Err(_) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
