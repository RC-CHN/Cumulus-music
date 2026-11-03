use super::{MusicClient, PlaySelection, PlayerView};
use crate::{
    Error, Result,
    account::SessionToken,
    client::focus::{FocusSnapshot, Owner},
    playback::{
        controller::{PlaybackCommand, PlaybackFence, PlaybackSnapshot},
        engine::Cancellation,
    },
};
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

const TICKET_LIFETIME: Duration = Duration::from_secs(60);

pub(super) struct PreparedSelection {
    id: String,
    token: SessionToken,
    fence: PlaybackFence,
    focus: FocusSnapshot,
    pub(super) observed: u64,
    generation: u64,
    issued: Instant,
}

impl PreparedSelection {
    fn check(&self, client: &MusicClient, current: &PlaybackSnapshot) -> Result<()> {
        if client.player.prepared_generation.load(Ordering::Acquire) != self.generation
            || self.issued.elapsed() >= TICKET_LIFETIME
            || current.account != self.token
            || current.fence() != self.fence
            || client.player.selection.load(Ordering::Acquire) != self.observed
        {
            return Err(Error::StaleResponse);
        }
        client.session.check_generation(&self.token)?;
        client.focus.check_snapshot(self.focus)
    }
}

impl MusicClient {
    /// Capture the native playback state before the renderer reads a card.
    /// One outstanding process-local ticket; starting a new one replaces it.
    pub fn prepare_play_selection(
        &self,
        epoch: &str,
        expected_queue_revision: &str,
        expected_play_id: Option<&str>,
    ) -> Result<String> {
        let token = self.player_token(epoch)?;
        let mut slot = self
            .player
            .prepared_selection
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let observed = self.player.selection.load(Ordering::Acquire);
        let focus = self.focus.snapshot()?;
        let current = controller.snapshot();
        if current.account != token
            || current.queue_revision.to_string() != expected_queue_revision
            || current.play_id.as_deref() != expected_play_id
        {
            return Err(Error::StaleResponse);
        }
        let previous = self.player.prepared_generation.load(Ordering::Acquire);
        let mut ticket = PreparedSelection {
            id: uuid::Uuid::new_v4().simple().to_string(),
            token,
            fence: current.fence(),
            focus,
            observed,
            generation: previous,
            issued: Instant::now(),
        };
        ticket.check(self, &controller.snapshot())?;
        ticket.generation = previous
            .checked_add(1)
            .ok_or(Error::State("playback preparation exhausted"))?;
        self.player
            .prepared_generation
            .store(ticket.generation, Ordering::Release);
        let id = ticket.id.clone();
        *slot = Some(ticket);
        Ok(id)
    }

    pub async fn commit_prepared_selection(
        &self,
        epoch: &str,
        id: &str,
        selection: PlaySelection,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        let ticket = {
            let mut slot = self
                .player
                .prepared_selection
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if !slot
                .as_ref()
                .is_some_and(|value| value.token == token && value.id == id)
            {
                return Err(Error::StaleResponse);
            }
            slot.take().ok_or(Error::StaleResponse)?
        };
        if !matches!(
            selection,
            PlaySelection::Catalog { .. }
                | PlaySelection::DailyRecommendations { .. }
                | PlaySelection::StyleRecommendations { .. }
                | PlaySelection::Podcast { .. }
                | PlaySelection::Voice { .. }
                | PlaySelection::Playlist { .. }
                | PlaySelection::SearchChart { .. }
                | PlaySelection::Songs { .. }
        ) {
            return Err(Error::InvalidInput(
                "unsupported prepared playback selection",
            ));
        }
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let current = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        ticket.check(self, &current)?;
        self.play_selection_inner(epoch, selection, Some(ticket))
            .await
    }

    pub(super) async fn submit_selection(
        &self,
        token: &SessionToken,
        command: PlaybackCommand,
        attempt: u64,
        lease: Option<u64>,
        prepared: Option<&PreparedSelection>,
    ) -> Result<PlaybackSnapshot> {
        self.submit_selection_with_source(token, command, attempt, lease, prepared, None)
            .await
    }

    pub(super) async fn submit_selection_with_source(
        &self,
        token: &SessionToken,
        command: PlaybackCommand,
        attempt: u64,
        lease: Option<u64>,
        prepared: Option<&PreparedSelection>,
        source_guard: Option<Cancellation>,
    ) -> Result<PlaybackSnapshot> {
        let check_source = || {
            if source_guard
                .as_ref()
                .is_some_and(Cancellation::is_cancelled)
            {
                return Err(Error::QueueChanged);
            }
            Ok(())
        };
        check_source()?;
        let controller = self.player.controller().await?;
        let Some(prepared) = prepared else {
            if self.player.selection.load(Ordering::Acquire) != attempt {
                return Err(Error::StaleResponse);
            }
            let lease = lease.ok_or(Error::StaleResponse)?;
            self.focus.check(lease, Owner::Audio)?;
            if let Some(source) = source_guard {
                let focus = self.focus.clone();
                let selection = self.player.selection.clone();
                return controller
                    .command_guarded(
                        token,
                        command,
                        controller.snapshot().fence(),
                        Cancellation::new(move || {
                            !source.is_cancelled()
                                && selection.load(Ordering::Acquire) == attempt
                                && focus.check(lease, Owner::Audio).is_ok()
                        }),
                    )
                    .await;
            }
            return controller.command(token, command).await;
        };

        // Sync an externally written queue before making any focus changes.
        let current = controller.command(token, PlaybackCommand::Snapshot).await?;
        let (lease, reserved) = {
            let _slot = self
                .player
                .prepared_selection
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            prepared.check(self, &current)?;
            check_source()?;
            self.reserve_prepared_audio(token, prepared.focus, || {
                if controller.snapshot().fence() != prepared.fence {
                    return Err(Error::StaleResponse);
                }
                let reserved = prepared
                    .observed
                    .checked_add(1)
                    .ok_or(Error::State("playback selection exhausted"))?;
                self.player
                    .selection
                    .compare_exchange(
                        prepared.observed,
                        reserved,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .map_err(|_| Error::StaleResponse)?;
                Ok(reserved)
            })?
        };
        self.finish_media_claim(token, Owner::Audio, lease).await?;
        let selection = self.player.selection.clone();
        let focus = self.focus.clone();
        let preparations = self.player.prepared_generation.clone();
        let generation = prepared.generation;
        let issued = prepared.issued;
        controller
            .command_guarded(
                token,
                command,
                prepared.fence.clone(),
                Cancellation::new(move || {
                    source_guard
                        .as_ref()
                        .is_none_or(|source| !source.is_cancelled())
                        && selection.load(Ordering::Acquire) == reserved
                        && preparations.load(Ordering::Acquire) == generation
                        && issued.elapsed() < TICKET_LIFETIME
                        && focus.check(lease, Owner::Audio).is_ok()
                }),
            )
            .await
    }
}
