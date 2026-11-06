//! Native shelf and guarded SongCard song actions (M/3085).
use super::{Seed, VoiceSongSelectionAction, VoiceSongSelectionKind, seed};
use crate::{
    Error, Result,
    account::SessionToken,
    client::{
        MusicClient,
        dto::{PlayerView, TrackView},
        focus::Owner,
        mapping,
        player::{
            identity, project,
            queue_playlist::{QUEUE_LIMIT, insert_collection_bound},
            vinyl_similar::stale_credentials,
        },
    },
    membership::Quality,
    playback::{
        controller::{PlaybackCommand, QueueTransport, QueueUpdate},
        engine::Cancellation,
    },
    protocol::api::Api,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex, Weak, atomic::Ordering},
    time::{Duration, Instant},
};

const RECEIPT_LIFETIME: Duration = Duration::from_secs(600);

#[derive(Clone, Copy, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VoiceSongAction {
    Cover { index: usize },
    DoubleClick { index: usize },
    NextPlay { index: usize },
}

enum SongRequest {
    Single(VoiceSongAction),
    Selected(VoiceSongSelectionAction),
}

impl SongRequest {
    fn index(&self) -> usize {
        match self {
            Self::Single(
                VoiceSongAction::Cover { index }
                | VoiceSongAction::DoubleClick { index }
                | VoiceSongAction::NextPlay { index },
            ) => *index,
            Self::Selected(selection) => selection.index,
        }
    }

    fn is_next(&self) -> bool {
        matches!(self, Self::Single(VoiceSongAction::NextPlay { .. }))
            || matches!(self, Self::Selected(action) if action.kind == VoiceSongSelectionKind::NextPlay)
    }

    fn is_double_click(&self) -> bool {
        matches!(self, Self::Single(VoiceSongAction::DoubleClick { .. }))
            || matches!(self, Self::Selected(action) if action.kind == VoiceSongSelectionKind::DoubleClick)
    }
}

pub(super) struct Row {
    pub(super) track: TrackView,
    pub(super) online_eligible: bool,
    pub(super) copyright_blocked: bool,
    pub(super) download: super::download::Class,
}

impl Row {
    pub(super) fn resource(&self, quality: Quality) -> Value {
        json!({"id":self.track.id,"level":quality.as_str(),"dt":self.track.duration_ms,
            "missing":false,"song":mapping::projected_song_metadata(&self.track)})
    }
}

pub(super) struct Page {
    pub(super) token: SessionToken,
    pub(super) play_id: String,
    pub(super) seed: Seed,
    pub(super) api: Weak<dyn Api>,
    pub(super) revision: String,
    pub(super) rows: Vec<Row>,
    pub(super) picker: super::picker::Shelf,
    pub(super) receipt: String,
    pub(super) created: Instant,
}

#[derive(Default)]
struct Slot {
    ticket: u64,
    page: Option<Arc<Page>>,
}

#[derive(Default)]
pub(in crate::client::player) struct Host(Mutex<Slot>);

impl Host {
    pub(super) fn start(&self, check: impl FnOnce() -> Result<()>) -> Result<u64> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        // A late stale read cannot replace a newer in-flight ticket.
        check()?;
        slot.ticket = slot
            .ticket
            .checked_add(1)
            .ok_or(Error::State("vinyl episode songs read exhausted"))?;
        Ok(slot.ticket)
    }

    pub(super) fn reading(&self, ticket: u64) -> Result<()> {
        let slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if slot.ticket != ticket {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    pub(super) fn install(
        &self,
        ticket: u64,
        page: Arc<Page>,
        check: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if slot.ticket != ticket {
            return Err(Error::StaleResponse);
        }
        check()?;
        slot.page = (!page.rows.is_empty()).then_some(page);
        Ok(())
    }

    pub(super) fn page(
        &self,
        token: &SessionToken,
        play_id: &str,
        receipt: &str,
    ) -> Result<Arc<Page>> {
        if receipt.len() != 32 || !receipt.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::StaleResponse);
        }
        self.0
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .page
            .as_ref()
            .filter(|page| {
                &page.token == token
                    && page.play_id == play_id
                    && page.receipt == receipt
                    && page.created.elapsed() <= RECEIPT_LIFETIME
            })
            .cloned()
            .ok_or(Error::StaleResponse)
    }

    fn current(&self, page: &Arc<Page>) -> bool {
        self.0.lock().is_ok_and(|slot| {
            slot.page
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, page))
                && page.created.elapsed() <= RECEIPT_LIFETIME
        })
    }

    pub(super) fn check_action_scope(
        &self,
        client: &MusicClient,
        page: &Arc<Page>,
        api: &Arc<dyn Api>,
        room: &Cancellation,
    ) -> Result<()> {
        client
            .session
            .with_verified_profile(&page.token, api, |_, _| Ok(()))?;
        if client.login_pending()?
            || room.is_cancelled()
            || !self.current(page)
            || api
                .verified_pc_experiment_revision()
                .map_err(stale_credentials)?
                .as_deref()
                != Some(page.revision.as_str())
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

impl MusicClient {
    pub async fn player_voice_song_play(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
        action: VoiceSongAction,
        quality: Quality,
    ) -> Result<PlayerView> {
        self.execute_voice_songs_action(
            epoch,
            play_id,
            receipt,
            SongRequest::Single(action),
            quality,
        )
        .await
    }

    pub async fn player_voice_songs_selection(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
        action: VoiceSongSelectionAction,
        quality: Quality,
    ) -> Result<PlayerView> {
        self.execute_voice_songs_action(
            epoch,
            play_id,
            receipt,
            SongRequest::Selected(action),
            quality,
        )
        .await
    }

    async fn execute_voice_songs_action(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
        request: SongRequest,
        quality: Quality,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        let host = self.player.voice_songs.clone();
        let page = host.page(&token, play_id, receipt)?;
        let api = page.api.upgrade().ok_or(Error::StaleResponse)?;
        let room = self.together_detached_guard(&token);
        let check = || host.check_action_scope(self, &page, &api, &room);
        check()?;
        let index = request.index();
        let is_next = request.is_next();
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let selection = self.player.selection.clone();
        let observed = selection.load(Ordering::Acquire);
        let focus = self.focus.snapshot()?;
        let _room_gate = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        let (before, queue) = controller.queue_with_state(&token).await?;
        if seed(&before, &token, play_id)? != page.seed {
            return Err(Error::StaleResponse);
        }
        if queue.intelligence.is_some() {
            return Err(Error::State(
                "vinyl episode songs require an ordinary queue",
            ));
        }
        let clicked = page
            .rows
            .get(index)
            .ok_or(Error::InvalidInput("invalid vinyl episode song row"))?;
        if !clicked.online_eligible || (is_next && clicked.copyright_blocked) {
            return Err(Error::State("vinyl episode song is unavailable online"));
        }
        let lease = focus.generation;
        if focus.owner != Some(Owner::Audio) || self.focus.check(lease, Owner::Audio).is_err() {
            return Err(Error::StaleResponse);
        }
        let fence = before.fence();
        let source = json!({"kind":"vinyl_voice_songs","id":page.seed.voice_id,"text":"包含歌曲","scene":"unknown"});
        let replace = request.is_double_click() && self.replace_queue_on_double_click(&token)?;
        // Current playback is always the validated episode, never an
        // ordinary song—even if both point to the same backing song ID.
        let command = if let SongRequest::Selected(action) = request {
            action.command(&page, quality, replace, queue, &fence, source)?
        } else if replace {
            PlaybackCommand::Replace {
                resources: page.rows.iter().map(|row| row.resource(quality)).collect(),
                index,
                source,
            }
        } else {
            if (is_next && queue.items.len() > QUEUE_LIMIT)
                || (queue.items.len() >= QUEUE_LIMIT
                    && !queue
                        .items
                        .iter()
                        .any(|entry| identity::online_song(&entry.resource, &clicked.track.id)))
            {
                return Err(Error::State("playback queue is full"));
            }
            let original = is_next.then(|| queue.clone());
            let mut queue = insert_collection_bound(
                queue,
                vec![clicked.resource(quality)],
                source,
                false,
                true,
                true,
            );
            if !is_next {
                queue.current = Some(
                    queue
                        .items
                        .iter()
                        .find(|row| identity::online_song(&row.resource, &clicked.track.id))
                        .ok_or(Error::State("vinyl episode song was not inserted"))?
                        .entry_id
                        .clone(),
                );
            }
            if original.as_ref() == Some(&queue) {
                // Repeating the same promotion is a guarded observation, not
                // another write, focus claim, or transport intent.
                PlaybackCommand::Snapshot
            } else {
                PlaybackCommand::UpdateQueue(QueueUpdate {
                    expected: fence.clone(),
                    queue,
                    transport: (!is_next).then_some(QueueTransport {
                        position: None,
                        paused: false,
                    }),
                })
            }
        };
        check()?;
        let attempt = if is_next {
            // A queue-only edit observes an existing audio intent. The
            // actor's queue/control fence rejects competing newer changes.
            observed
        } else {
            let attempt = observed
                .checked_add(1)
                .ok_or(Error::State("playback selection exhausted"))?;
            selection
                .compare_exchange(observed, attempt, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| Error::StaleResponse)?;
            attempt
        };
        check()?;
        let focus = self.focus.clone();
        let session = self.session.clone();
        let captured = token.clone();
        let result = controller
            .command_guarded(
                &token,
                command,
                fence,
                Cancellation::new(move || {
                    selection.load(Ordering::Acquire) == attempt
                        && focus.check(lease, Owner::Audio).is_ok()
                        && !room.is_cancelled()
                        && host.current(&page)
                        && session
                            .with_verified_profile(&captured, &api, |_, _| Ok(()))
                            .is_ok()
                        && api.verified_pc_experiment_revision().is_ok_and(|revision| {
                            revision.as_deref() == Some(page.revision.as_str())
                        })
                }),
            )
            .await?;
        self.session.check_generation(&token)?;
        project(&result, &token)
    }
}
