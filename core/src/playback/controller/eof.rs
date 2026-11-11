//! M308 natural EOF, separately from manual transport, preload and decoder errors.
use super::{
    Message, PlaybackFence, PlaybackStatus,
    load::{LoadOptions, LoadSource, OrdinaryEofDownload},
    state::Actor,
};
use crate::{
    Error, Result,
    account::SessionToken,
    playback::{
        PlaybackMode, QueueEntry, QueueSnapshot, Step, TimelineMode,
        engine::{Cancellation, MediaInput},
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

pub(super) struct EndedMinibar {
    receipt: Uuid,
    native_entry: i64,
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
    ControlKeepFinished,
    StopFinished,
    PlayExisting {
        target: String,
        notify: bool,
    },
    PlayDownload {
        entry: Box<QueueEntry>,
        held: Box<OrdinaryEofDownload>,
        skipped: Vec<String>,
    },
}

pub(super) struct Ready {
    id: Uuid,
    ticket: Ticket,
    result: Result<Outcome>,
}

impl Actor {
    pub(super) fn ended_minibar_receipt(
        &self,
        selected: Option<&crate::playback::QueueEntry>,
    ) -> Option<Uuid> {
        let ended = self.ended_minibar.as_ref()?;
        let loaded = self.loaded.as_ref()?;
        // The original EOF mode governs issuance, not continued ownership.
        // A mode-only edit changes the queue revision and manual adjacency,
        // but the same finished native entry still has no play ID.
        (self.state == PlaybackStatus::Ended
            && self.play_id.is_none()
            && loaded.media.entry_id == ended.native_entry
            && selected == Some(&loaded.item)
            && self.ordinary_online_song(&loaded.item)
            && loaded.source.info["kind"] == "online"
            && loaded.timeline.snapshot().mode == TimelineMode::Full)
            .then_some(ended.receipt)
    }

    /// Invoked only after a matching successful native EOF has been accepted
    /// and the old row's completion saved. A generic host has no admission.
    pub(super) fn schedule_ordinary_eof(&mut self, was_paused: bool) -> bool {
        let (Some(provider), Some(loaded)) = (&self.ordinary_eof, &self.loaded) else {
            return false;
        };
        if !matches!(
            self.queue.mode(),
            PlaybackMode::Sequential | PlaybackMode::OrderLoop | PlaybackMode::Random
        ) || self.queue.len() > crate::playback::intelligence::MAX_QUEUE_TRACKS
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
            || (queue.mode != PlaybackMode::Random
                && !queue.items.iter().map(|row| &row.entry_id).eq(&queue.order))
            || queue.items.iter().any(|row| {
                // This branch may discover an exact same-song native
                // download, never an alternate track, video or explicit
                // local/room resource in the supplied queue.
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
                    return Ok(Outcome::PlayExisting {
                        target: attempted.entry_id,
                        notify: false,
                    });
                }
                if !verified.groups.skip_songs_assigned() {
                    return Ok(Outcome::ControlKeepFinished);
                }
                let scan = resolver
                    .ordinary_queue_preflight(&ticket.token, &ticket.queue, ENVIRONMENT)
                    .await?;
                ticket.authority.check()?;
                if scan.rows.iter().any(|row| !row.no_download_publication) {
                    if ticket.queue.mode != PlaybackMode::Sequential {
                        return Ok(Outcome::KeepFinished);
                    }
                    let OrdinarySkipDecision::PlayExisting {
                        target,
                        denial_count,
                        ..
                    } = skip_policy::after_initial_denial_with_download(
                        &ticket.queue,
                        &scan,
                        TRIGGER,
                        true,
                    )?
                    else {
                        return Ok(Outcome::KeepFinished);
                    };
                    let current = ticket
                        .queue
                        .items
                        .iter()
                        .position(|row| Some(&row.entry_id) == ticket.queue.current.as_ref())
                        .ok_or(Error::QueueChanged)?;
                    let next = current + 1 + denial_count;
                    let entry = ticket
                        .queue
                        .items
                        .get(next)
                        .filter(|row| row.entry_id == target)
                        .ok_or(Error::QueueChanged)?;
                    let Some(publication) =
                        scan.rows[next].publication.clone().filter(|p| !p.encrypted)
                    else {
                        return Ok(Outcome::KeepFinished);
                    };
                    let skipped = scan.rows[current + 1..next]
                        .iter()
                        .map(|row| row.original_song_id.clone())
                        .collect();
                    let (source, file) = resolver
                        .hold_ordinary_eof_download(
                            &ticket.token,
                            entry.clone(),
                            publication,
                            &ticket.authority,
                        )
                        .await?;
                    ticket.authority.check()?;
                    return Ok(Outcome::PlayDownload {
                        entry: Box::new(entry.clone()),
                        held: Box::new(OrdinaryEofDownload { source, file }),
                        skipped,
                    });
                }
                Ok(
                    match skip_policy::after_initial_denial(&ticket.queue, &scan, TRIGGER)? {
                        OrdinarySkipDecision::StopAtDeniedTail { .. }
                        | OrdinarySkipDecision::StopAtErrorLimit { .. } => Outcome::StopFinished,
                        OrdinarySkipDecision::PlayExisting { target, .. } => {
                            Outcome::PlayExisting {
                                target,
                                notify: true,
                            }
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
            Ok(Outcome::ControlKeepFinished) => {
                // M308's PlayOrder branch returns; PlayRandom/playCycle fall
                // through onPlayEnd and clear the old playId (3833–3838).
                // Keep the completed row and timeline; no new engine command.
                if matches!(queue.mode, PlaybackMode::OrderLoop | PlaybackMode::Random) {
                    self.play_id = None;
                    self.ended_minibar = Some(EndedMinibar {
                        receipt: Uuid::new_v4(),
                        native_entry,
                    });
                }
            }
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
            Ok(Outcome::PlayExisting { target, notify }) => {
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
                if notify {
                    self.publish_ordinary_skip();
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
            Ok(Outcome::PlayDownload {
                entry,
                held,
                skipped,
            }) => {
                let MediaInput::SearchPublication(publication) = &held.source.input else {
                    return Ok(());
                };
                // An unavailable decoder or stale filesystem identity must
                // leave the exact prior ended row untouched. A read-only
                // capability check cannot stop a decoder on cancellation.
                if !self
                    .engine
                    .try_lock()
                    .is_ok_and(|engine| engine.supports_pinned_search_download())
                    || held.file.check_ordinary_eof_file(&entry).is_err()
                    || authority.is_cancelled()
                {
                    return Ok(());
                }
                if self
                    .session
                    .with_token(&token, |owner| {
                        self.queue.select_ordinary_eof_download(
                            owner,
                            &entry,
                            publication,
                            &skipped,
                        )
                    })
                    .is_err()
                {
                    return Ok(());
                }
                self.publish_ordinary_skip();
                self.begin(
                    LoadOptions {
                        paused: was_paused,
                        source: LoadSource::OrdinaryEofDownload(held),
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
