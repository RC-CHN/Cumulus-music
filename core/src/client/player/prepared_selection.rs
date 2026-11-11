use super::{MusicClient, PlaySelection, PlayerView};
use crate::{
    Error, Result,
    account::SessionToken,
    client::focus::{FocusSnapshot, Owner},
    playback::{
        controller::{
            HistoryQueueUpdate, LocalBatchUpdate, PlaybackCommand, PlaybackFence, PlaybackSnapshot,
            QueueUpdate,
        },
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
    room_authority: Option<Cancellation>,
}

impl PreparedSelection {
    pub(super) fn check(&self, client: &MusicClient, current: &PlaybackSnapshot) -> Result<()> {
        if client.player.prepared_generation.load(Ordering::Acquire) != self.generation
            || self.issued.elapsed() >= TICKET_LIFETIME
            || current.account != self.token
            || current.fence() != self.fence
            || client.player.selection.load(Ordering::Acquire) != self.observed
            || self
                .room_authority
                .as_ref()
                .is_some_and(Cancellation::is_cancelled)
        {
            return Err(Error::StaleResponse);
        }
        client.session.check_generation(&self.token)?;
        client.focus.check_snapshot(self.focus)
    }
}

#[derive(Clone, Copy)]
enum NativeFileAdmission {
    Legacy,
    #[cfg(windows)]
    RootedWindows,
}

impl MusicClient {
    pub(super) async fn stage_together_replacement(
        &self,
        token: &SessionToken,
        room_authority: Cancellation,
    ) -> Result<PreparedSelection> {
        let observed = self.player.selection.load(Ordering::Acquire);
        let focus = self.focus.snapshot()?;
        let actor = self.player.controller().await?;
        let current = actor.command(token, PlaybackCommand::Snapshot).await?;
        let _slot = self
            .player
            .prepared_selection
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        if self.player.selection.load(Ordering::Acquire) != observed {
            return Err(Error::StaleResponse);
        }
        let generation = self
            .player
            .prepared_generation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| Error::State("playback preparation generation exhausted"))?
            + 1;
        Ok(PreparedSelection {
            id: uuid::Uuid::new_v4().simple().to_string(),
            token: token.clone(),
            fence: current.fence(),
            focus,
            observed,
            generation,
            issued: Instant::now(),
            room_authority: Some(room_authority),
        })
    }

    pub(super) fn pending_preparation_guard(
        &self,
        prepared: &PreparedSelection,
        controller: std::sync::Arc<crate::playback::controller::PlaybackController>,
    ) -> Cancellation {
        let generation = self.player.prepared_generation.clone();
        let selection = self.player.selection.clone();
        let focus = self.focus.clone();
        let (expected, owner, observed, sequence, issued) = (
            prepared.fence.clone(),
            prepared.focus,
            prepared.observed,
            prepared.generation,
            prepared.issued,
        );
        Cancellation::new(move || {
            generation.load(Ordering::Acquire) == sequence
                && selection.load(Ordering::Acquire) == observed
                && issued.elapsed() < TICKET_LIFETIME
                && controller.snapshot().fence() == expected
                && focus.check_snapshot(owner).is_ok()
        })
    }

    pub(super) async fn submit_prepared_queue_only(
        &self,
        token: &SessionToken,
        prepared: &PreparedSelection,
        update: QueueUpdate,
        source: Cancellation,
    ) -> Result<PlaybackSnapshot> {
        if update.transport.is_some() {
            return Err(Error::StaleResponse);
        }
        self.submit_prepared_queue_edit(
            token,
            prepared,
            update.expected.clone(),
            PlaybackCommand::UpdateQueue(update),
            source,
        )
        .await
    }

    pub(super) async fn submit_prepared_local_queue_only(
        &self,
        token: &SessionToken,
        prepared: &PreparedSelection,
        update: LocalBatchUpdate,
        source: Cancellation,
    ) -> Result<PlaybackSnapshot> {
        if update.start {
            return Err(Error::StaleResponse);
        }
        self.submit_prepared_queue_edit(
            token,
            prepared,
            update.expected.clone(),
            PlaybackCommand::ApplyLocalBatch(update),
            source,
        )
        .await
    }

    pub(super) async fn submit_prepared_history_queue_only(
        &self,
        token: &SessionToken,
        prepared: &PreparedSelection,
        update: HistoryQueueUpdate,
        source: Cancellation,
    ) -> Result<PlaybackSnapshot> {
        if update.start {
            return Err(Error::StaleResponse);
        }
        self.submit_prepared_queue_edit(
            token,
            prepared,
            update.expected.clone(),
            PlaybackCommand::ApplyHistoryBatch(Box::new(update)),
            source,
        )
        .await
    }

    async fn submit_prepared_queue_edit(
        &self,
        token: &SessionToken,
        prepared: &PreparedSelection,
        expected: PlaybackFence,
        command: PlaybackCommand,
        source: Cancellation,
    ) -> Result<PlaybackSnapshot> {
        if expected != prepared.fence {
            return Err(Error::StaleResponse);
        }
        let controller = self.player.controller().await?;
        let current = controller.command(token, PlaybackCommand::Snapshot).await?;
        prepared.check(self, &current)?;
        source.check()?;
        let selection = self.player.selection.clone();
        let focus = self.focus.clone();
        let preparations = self.player.prepared_generation.clone();
        let (attempt, generation, issued, owner) = (
            prepared.observed,
            prepared.generation,
            prepared.issued,
            prepared.focus,
        );
        controller
            .command_guarded(
                token,
                command,
                prepared.fence.clone(),
                Cancellation::new(move || {
                    !source.is_cancelled()
                        && selection.load(Ordering::Acquire) == attempt
                        && preparations.load(Ordering::Acquire) == generation
                        && issued.elapsed() < TICKET_LIFETIME
                        && focus.check_snapshot(owner).is_ok()
                }),
            )
            .await
    }

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
            room_authority: None,
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
        let ticket = self.take_prepared_selection(&token, id)?;
        if !matches!(
            selection,
            PlaySelection::Catalog { .. }
                | PlaySelection::DailyRecommendations { .. }
                | PlaySelection::DailyRecommendationsBatch { .. }
                | PlaySelection::StyleRecommendations { .. }
                | PlaySelection::Podcast { .. }
                | PlaySelection::LocalBatch { .. }
                | PlaySelection::LocalBatchAdd { .. }
                | PlaySelection::PodcastSubscriptionRecommendation { .. }
                | PlaySelection::Voice { .. }
                | PlaySelection::Playlist { .. }
                | PlaySelection::SearchChart { .. }
                | PlaySelection::SearchArtistCard { .. }
                | PlaySelection::SearchPodcastCard { .. }
                | PlaySelection::SearchPodcastRow { .. }
                | PlaySelection::SearchVoiceMenu { .. }
                | PlaySelection::PodcastBrowseVoice { .. }
                | PlaySelection::PodcastPurchasedEpisode { .. }
                | PlaySelection::PodcastLikedEpisode { .. }
                | PlaySelection::PodcastLikedPageEpisode { .. }
                | PlaySelection::HistoryRadioChapter { .. }
                | PlaySelection::HistoryRadioMenu { .. }
                | PlaySelection::HistoryAdd { .. }
                | PlaySelection::HistoryAddAll { .. }
                | PlaySelection::HistoryPlayAll { .. }
                | PlaySelection::HistoryBatchPlay { .. }
                | PlaySelection::HistoryRowNext { .. }
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

    fn take_prepared_selection(&self, token: &SessionToken, id: &str) -> Result<PreparedSelection> {
        let mut slot = self
            .player
            .prepared_selection
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        if !slot
            .as_ref()
            .is_some_and(|value| &value.token == token && value.id == id)
        {
            return Err(Error::StaleResponse);
        }
        slot.take().ok_or(Error::StaleResponse)
    }

    /// Trusted cold desktop launch only; no WebView command takes these paths.
    pub async fn play_native_open_files(
        &self,
        epoch: &str,
        paths: Vec<std::path::PathBuf>,
    ) -> Result<PlayerView> {
        self.open_native_files(epoch, paths, true, None, NativeFileAdmission::Legacy)
            .await
    }

    /// Trusted same-user native relay only. Uses the live selection fence and
    /// never exposes path input to a renderer command.
    pub async fn play_native_open_files_warm(
        &self,
        epoch: &str,
        paths: Vec<std::path::PathBuf>,
        authority: Cancellation,
    ) -> Result<PlayerView> {
        self.open_native_files(
            epoch,
            paths,
            false,
            Some(authority),
            NativeFileAdmission::Legacy,
        )
        .await
    }

    /// Candidate Windows cold entry: paths come only from native shell input.
    /// The native readiness predicate has the same constraints as for warm.
    #[cfg(windows)]
    pub async fn play_windows_native_open_files(
        &self,
        epoch: &str,
        paths: Vec<std::path::PathBuf>,
        authority: Cancellation,
    ) -> Result<PlayerView> {
        self.open_native_files(
            epoch,
            paths,
            true,
            Some(authority),
            NativeFileAdmission::RootedWindows,
        )
        .await
    }

    /// Candidate Windows warm entry. The source authority is a cheap native
    /// window/relay check, including at the final queue commit. Its predicate
    /// must not access the account session, playback host, or state database.
    #[cfg(windows)]
    pub async fn play_windows_native_open_files_warm(
        &self,
        epoch: &str,
        paths: Vec<std::path::PathBuf>,
        authority: Cancellation,
    ) -> Result<PlayerView> {
        self.open_native_files(
            epoch,
            paths,
            false,
            Some(authority),
            NativeFileAdmission::RootedWindows,
        )
        .await
    }

    async fn open_native_files(
        &self,
        epoch: &str,
        paths: Vec<std::path::PathBuf>,
        cold: bool,
        authority: Option<Cancellation>,
        admission: NativeFileAdmission,
    ) -> Result<PlayerView> {
        use crate::{
            library::{
                native_open::{MAX_NATIVE_FILES, inspect_opened_file},
                probe::Ffprobe,
            },
            playback::controller::PlaybackStatus,
        };
        if let Some(authority) = &authority {
            authority.check()?;
        }
        let token = self.player_token(epoch)?;
        let controller = self.player.controller().await?;
        let current = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        if cold && (current.state != PlaybackStatus::Idle || current.play_id.is_some()) {
            return Err(Error::StaleResponse);
        }
        // Even an invalid explicit file must not accidentally start the
        // restored online queue. Retire only this snapshot's startup latch.
        let current = if cold && current.startup_pending {
            controller
                .command(&token, PlaybackCommand::SkipStartup(current.fence()))
                .await?
        } else {
            current
        };
        if paths.is_empty() || paths.len() > MAX_NATIVE_FILES {
            return Err(Error::InvalidInput("no supported native local audio files"));
        }
        let id = self.prepare_play_selection(
            epoch,
            &current.queue_revision.to_string(),
            current.play_id.as_deref(),
        )?;
        let ticket = self.take_prepared_selection(&token, &id)?;
        let session = self.session.clone();
        let worker_token = token.clone();
        let worker_authority = authority.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let probe = Ffprobe::for_native_open();
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut resources = Vec::new();
            #[cfg(windows)]
            let mut rooted = crate::library::WindowsNativeOpenBatch::new();
            for path in paths {
                session.check_generation(&worker_token)?;
                if let Some(authority) = &worker_authority {
                    authority.check()?;
                }
                if Instant::now() >= deadline {
                    break;
                }
                match admission {
                    NativeFileAdmission::Legacy => {
                        if let Ok(resource) = inspect_opened_file(&path, &probe) {
                            resources.push(resource);
                        }
                    }
                    #[cfg(windows)]
                    NativeFileAdmission::RootedWindows => {
                        let _ = rooted.push(path, &probe);
                    }
                }
            }
            #[cfg(windows)]
            if matches!(admission, NativeFileAdmission::RootedWindows) && !rooted.is_empty() {
                return Ok(PlaybackCommand::ReplaceWindowsNative(rooted));
            }
            if resources.is_empty() {
                return Err(Error::InvalidInput("no supported native local audio files"));
            }
            Ok(PlaybackCommand::Replace {
                resources,
                index: 0,
                source: serde_json::json!({"kind":"local", "nativeOpen":true}),
            })
        });
        let command = tokio::time::timeout(Duration::from_secs(12), worker)
            .await
            .map_err(|_| Error::Media("native local audio probe timed out"))?
            .map_err(|_| Error::State("native local audio worker stopped"))??;
        let state = self
            .submit_selection_with_source(
                &token,
                command,
                ticket.observed,
                None,
                Some(&ticket),
                authority,
            )
            .await?;
        self.session.check_generation(&token)?;
        super::project(&state, &token)
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
        if source_guard
            .as_ref()
            .is_some_and(Cancellation::is_cancelled)
        {
            return Err(Error::QueueChanged);
        }
        if let Some(prepared) = prepared {
            if matches!(&command, PlaybackCommand::Replace { .. })
                && self.together_external_room(token)?.is_some()
            {
                let actor = self.player.controller().await?;
                let current = actor.command(token, PlaybackCommand::Snapshot).await?;
                prepared.check(self, &current)?;
            }
        } else if self.player.selection.load(Ordering::Acquire) != attempt {
            return Err(Error::StaleResponse);
        }
        let source_guard = if let Some(room) = prepared.and_then(|p| p.room_authority.clone()) {
            Some(Cancellation::new(move || {
                !room.is_cancelled()
                    && source_guard
                        .as_ref()
                        .is_none_or(|source| !source.is_cancelled())
            }))
        } else {
            source_guard
        };
        let (command, together) = self
            .filter_together_external(
                token,
                command,
                prepared.map(|p| &p.fence),
                source_guard.as_ref(),
            )
            .await?;
        let source_guard = if let Some(filtered) = &together {
            let room = self.filtered_external_authority(filtered);
            Some(Cancellation::new(move || {
                !room.is_cancelled()
                    && source_guard
                        .as_ref()
                        .is_none_or(|source| !source.is_cancelled())
            }))
        } else {
            source_guard
        };
        let result = match (command, prepared, together.as_ref()) {
            (
                command @ PlaybackCommand::ApplyTogetherExternalQueue(_),
                Some(prepared),
                Some(room),
            ) => {
                self.submit_prepared_queue_edit(
                    token,
                    prepared,
                    room.fence.clone(),
                    command,
                    source_guard.ok_or(Error::StaleResponse)?,
                )
                .await
            }
            (command, prepared, together) => {
                self.submit_selection_authorized(
                    token,
                    command,
                    attempt,
                    lease,
                    prepared,
                    source_guard,
                    together.map(|scope| &scope.fence),
                )
                .await
            }
        };
        if result.is_ok()
            && let Some(together) = together
        {
            let _ = self.finish_external_filter(together);
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn submit_selection_authorized(
        &self,
        token: &SessionToken,
        command: PlaybackCommand,
        attempt: u64,
        lease: Option<u64>,
        prepared: Option<&PreparedSelection>,
        source_guard: Option<Cancellation>,
        external_fence: Option<&crate::playback::controller::PlaybackFence>,
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
            if source_guard.is_some() || external_fence.is_some() {
                // The last published view can still belong to the prior
                // account. Bind and sync the actor before taking its fence.
                let current = controller.command(token, PlaybackCommand::Snapshot).await?;
                if source_guard
                    .as_ref()
                    .is_some_and(Cancellation::is_cancelled)
                    || external_fence.is_some_and(|expected| current.fence() != *expected)
                {
                    return Err(Error::QueueChanged);
                }
                if self.player.selection.load(Ordering::Acquire) != attempt {
                    return Err(Error::StaleResponse);
                }
                self.focus.check(lease, Owner::Audio)?;
                let focus = self.focus.clone();
                let selection = self.player.selection.clone();
                return controller
                    .command_guarded(
                        token,
                        command,
                        current.fence(),
                        Cancellation::new(move || {
                            source_guard
                                .as_ref()
                                .is_none_or(|source| !source.is_cancelled())
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
        if external_fence.is_some_and(|expected| current.fence() != *expected) {
            return Err(Error::QueueChanged);
        }
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
