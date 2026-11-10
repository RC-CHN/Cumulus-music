//! Single-program actions in the mixed purchase and liked tables.
use super::{
    MusicClient, PlaybackCommand, PlaybackStatus, PlayerResourceKind, identity,
    podcast_browse::{resource, voice_replacement_mode},
    prepared_selection::PreparedSelection,
    queue_playlist::{QUEUE_LIMIT, insert_collection_bound},
    voice_detail::bound_episode,
};
use crate::{
    Error, Result,
    account::SessionToken,
    client::podcast::{
        PodcastLikedPageTarget, PodcastLikedProgramTarget, PodcastPurchasedTarget,
        liked_menu::liked_playable_program, purchased::purchased_playback_episode,
    },
    playback::{
        PlaybackMode, QueueSnapshot,
        controller::{QueueTransport, QueueUpdate},
        engine::Cancellation,
    },
    podcast::{Episode, PodcastApi, PodcastLibrary},
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};

pub(super) fn purchased_origin() -> Value {
    json!({"kind":"podcast_purchased","scene":"myPayedAudio"})
}

pub(super) enum ScopedProgram<'a> {
    Purchased(&'a PodcastPurchasedTarget),
    Liked(&'a PodcastLikedProgramTarget),
    PagedLiked(&'a PodcastLikedPageTarget),
}

pub(super) struct PagedLikedProgram {
    pub row: Episode,
    pub prefix: Option<Vec<Episode>>,
    pub api: Arc<dyn Api>,
    pub guard: Cancellation,
}

impl ScopedProgram<'_> {
    pub(super) fn origin(&self) -> Value {
        match self {
            Self::Purchased(_) => purchased_origin(),
            Self::Liked(target) => {
                json!({"kind":"podcast","scope":{"kind":"liked"},"scene":"myLikedVoice","revision":target.revision})
            }
            Self::PagedLiked(target) => {
                json!({"kind":"podcast","scope":{"kind":"liked"},"scene":"myLikedVoice","revision":target.revision})
            }
        }
    }
}

fn compatible(before: &Episode, detail: &Episode) -> bool {
    detail.id == before.id
        && before
            .song_id
            .as_ref()
            .is_none_or(|song| detail.song_id.as_ref() == Some(song))
        && before.radio.as_ref().is_none_or(|expected| {
            detail
                .radio
                .as_ref()
                .is_some_and(|radio| radio.id == expected.id)
        })
        && detail.raw["grayDisplay"] != true
        && !detail.display_status_withdrawn()
}

fn same_online_voice(value: &Value, episode: &Episode, allow_unknown_song: bool) -> bool {
    identity::kind(value) == PlayerResourceKind::Voice
        && bound_episode(value).is_some_and(|(id, song, voice)| {
            id == episode.id
                && episode
                    .song_id
                    .as_ref()
                    .map_or(allow_unknown_song, |expected| expected == &song)
                && episode
                    .radio
                    .as_ref()
                    .is_none_or(|radio| voice["audio"]["id"] == radio.id)
        })
}

impl MusicClient {
    pub(super) fn paged_liked_episode_scope(
        &self,
        token: &SessionToken,
        target: &PodcastLikedPageTarget,
        limit: Option<usize>,
    ) -> Result<PagedLikedProgram> {
        let api = self.session.native_api(token)?;
        let credential = api.playback_source_credential_revision()?;
        let selected = self.liked_page_playback(token, target, limit)?;
        let session = self.session.clone();
        let owner = token.clone();
        let expected = api.clone();
        let login = self.auth.pending_guard();
        let room = self.together_detached_guard(token);
        let authority = selected.authority;
        let guard = Cancellation::new(move || {
            !authority.is_cancelled()
                && login().is_ok()
                && !room.is_cancelled()
                && session.require_api(&owner, &expected).is_ok()
                && expected
                    .playback_source_credential_revision()
                    .is_ok_and(|value| value == credential)
        });
        guard.check()?;
        Ok(PagedLikedProgram {
            row: selected.row,
            prefix: selected.prefix,
            api,
            guard,
        })
    }

    pub(super) fn liked_episode_scope(
        &self,
        token: &SessionToken,
        target: &PodcastLikedProgramTarget,
    ) -> Result<(Episode, Arc<dyn Api>, Cancellation)> {
        let api = self.session.native_api(token)?;
        let credential = api.playback_source_credential_revision()?;
        let row = liked_playable_program(&self.session, token, target)?;
        let session = self.session.clone();
        let owner = token.clone();
        let expected = api.clone();
        let target = target.clone();
        let login = self.auth.pending_guard();
        let room = self.together_detached_guard(token);
        let guard = Cancellation::new(move || {
            login().is_ok()
                && !room.is_cancelled()
                && session.require_api(&owner, &expected).is_ok()
                && expected
                    .playback_source_credential_revision()
                    .is_ok_and(|value| value == credential)
                && liked_playable_program(&session, &owner, &target).is_ok()
        });
        guard.check()?;
        Ok((row, api, guard))
    }
    pub(super) fn purchased_episode_scope(
        &self,
        token: &SessionToken,
        target: &PodcastPurchasedTarget,
    ) -> Result<(Episode, Arc<dyn Api>, Cancellation)> {
        let api = self.session.native_api(token)?;
        let credential = api.playback_source_credential_revision()?;
        let cached = purchased_playback_episode(&self.session, token, target)?;
        let library = PodcastLibrary::new(self.session.clone())?;
        let owner = token.clone();
        let revision = target.revision.clone();
        let session = self.session.clone();
        let bound = api.clone();
        let login = self.auth.pending_guard();
        let room = self.together_detached_guard(token);
        let guard = Cancellation::new(move || {
            login().is_ok()
                && !room.is_cancelled()
                && session.require_api(&owner, &bound).is_ok()
                && bound
                    .playback_source_credential_revision()
                    .is_ok_and(|current| current == credential)
                && library
                    .purchase_observation_is_current(&owner, &revision)
                    .unwrap_or(false)
        });
        guard.check()?;
        Ok((cached, api, guard))
    }

    pub(super) async fn read_scoped_program_detail(
        &self,
        token: &SessionToken,
        cached: &Episode,
        guard: &Cancellation,
    ) -> Result<Episode> {
        guard.check()?;
        let detail = self
            .session
            .run_for(token, |api, _| async {
                PodcastApi::new(api).episode(&json!(cached.id)).await
            })
            .await?;
        guard.check()?;
        if !compatible(cached, &detail) {
            return Err(Error::QueueChanged);
        }
        Ok(detail)
    }

    pub(super) async fn hydrate_paged_liked_episode(
        &self,
        token: &SessionToken,
        card: Episode,
        guard: &Cancellation,
    ) -> Result<Episode> {
        guard.check()?;
        let episode = if identifier(&card.raw["mainSong"]["id"]).is_ok() {
            card
        } else {
            match self.read_scoped_program_detail(token, &card, guard).await {
                Err(error)
                    if error.is_service_code(404)
                        || matches!(error.root(), Error::HttpStatus { status: 404, .. }) =>
                {
                    return Err(Error::State("liked voice media unavailable"));
                }
                reply => reply?,
            }
        };
        if episode.song_id.is_none() {
            return Err(Error::State("liked voice media unavailable"));
        }
        Ok(episode)
    }

    pub(super) async fn scoped_program_selection(
        &self,
        token: &SessionToken,
        target: ScopedProgram<'_>,
        menu: bool,
        toggle_current: bool,
        prepared: &PreparedSelection,
    ) -> Result<(PlaybackCommand, Cancellation)> {
        if toggle_current && !menu {
            return Err(Error::InvalidInput("conflicting liked episode gestures"));
        }
        let replace = matches!(target, ScopedProgram::PagedLiked(_))
            && !menu
            && self.replace_queue_on_double_click(token)?;
        let (cached, api, guard, prefix) = match target {
            ScopedProgram::Purchased(target) => {
                let (row, api, guard) = self.purchased_episode_scope(token, target)?;
                (row, api, guard, None)
            }
            ScopedProgram::Liked(target) => {
                let (row, api, guard) = self.liked_episode_scope(token, target)?;
                (row, api, guard, None)
            }
            ScopedProgram::PagedLiked(target) => {
                let selected =
                    self.paged_liked_episode_scope(token, target, replace.then_some(QUEUE_LIMIT))?;
                (selected.row, selected.api, selected.guard, selected.prefix)
            }
        };
        let controller = self.player.controller().await?;
        let (before, mut queue) = controller.queue_with_state(token).await?;
        prepared.check(self, &before)?;
        let pending = self.pending_preparation_guard(prepared, controller.clone());
        let preparing = {
            let guard = guard.clone();
            Cancellation::new(move || !guard.is_cancelled() && !pending.is_cancelled())
        };
        preparing.check()?;
        if before.fm || queue.intelligence.is_some() {
            return Err(Error::State("scoped episode requires ordinary playback"));
        }
        let loaded = |episode: &Episode, allow_unknown_song| {
            before.play_id.is_some()
                && matches!(
                    before.state,
                    PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
                )
                && before.selected.as_ref().is_some_and(|entry| {
                    same_online_voice(&entry.resource, episode, allow_unknown_song)
                })
        };
        if toggle_current {
            // The index Pause control is allowed to touch only the entry the
            // prepared ticket still observes. A miss can never start audio.
            if !loaded(&cached, true) {
                return Err(Error::QueueChanged);
            }
            return Ok((
                PlaybackCommand::TogglePodcastRank {
                    expected: before.fence(),
                    paused: !before.paused,
                    mode: None,
                },
                guard,
            ));
        }
        let card_category = cached.raw["categoryId"].clone();
        let replacement_mode = |episode: &Episode, old_mode| {
            voice_replacement_mode(
                episode.raw.get("categoryId").unwrap_or(&card_category),
                old_mode,
            )
        };
        let current_command = |episode: &Episode| {
            if matches!(target, ScopedProgram::PagedLiked(_)) && (!menu || before.paused) {
                let mode = replace
                    .then(|| replacement_mode(episode, queue.mode))
                    .filter(|mode| *mode != queue.mode);
                PlaybackCommand::TogglePodcastRank {
                    expected: before.fence(),
                    paused: !before.paused,
                    mode,
                }
            } else if !menu {
                PlaybackCommand::Pause(!before.paused)
            } else if before.paused {
                PlaybackCommand::Pause(false)
            } else {
                PlaybackCommand::Snapshot
            }
        };
        if identifier(&cached.raw["mainSong"]["id"]).is_ok() && loaded(&cached, false) {
            return Ok((current_command(&cached), guard));
        }

        let detail = if matches!(target, ScopedProgram::PagedLiked(_)) {
            self.hydrate_paged_liked_episode(token, cached, &preparing)
                .await?
        } else if matches!(target, ScopedProgram::Purchased(_))
            || identifier(&cached.raw["mainSong"]["id"]).is_err()
        {
            self.read_scoped_program_detail(token, &cached, &preparing)
                .await?
        } else {
            cached
        };
        if loaded(&detail, false) {
            return Ok((current_command(&detail), guard));
        }

        let selected = resource(&detail)?;
        let resources = match prefix {
            Some(rows) => {
                if !rows.iter().any(|row| row.id == detail.id) {
                    return Err(Error::QueueChanged);
                }
                rows.iter()
                    .map(|row| {
                        if row.id == detail.id {
                            Ok(selected.clone())
                        } else {
                            resource(row)
                        }
                    })
                    .collect::<Result<Vec<_>>>()?
            }
            None => vec![selected.clone()],
        };
        let position = self
            .podcast_browse_position(token, &api, &detail, &selected, &preparing)
            .await?;
        preparing.check()?;
        let first_source = controller
            .source_resolver()
            .prepare_scoped_episode(&selected, token, &api, &preparing, Some(&detail))
            .await?;
        preparing.check()?;
        let timeline = first_source.timeline(None)?;
        let position = position
            .filter(|position| timeline.to_media(*position).is_ok())
            .unwrap_or_else(|| timeline.snapshot().start);

        let source = target.origin();
        if replace {
            queue = QueueSnapshot {
                mode: replacement_mode(&detail, queue.mode),
                ..QueueSnapshot::default()
            };
        }
        queue = insert_collection_bound(queue, resources, source.clone(), false, true, true);
        let entry = queue
            .items
            .iter()
            .find(|entry| entry.resource == selected && entry.source == source)
            .map(|entry| entry.entry_id.clone())
            .ok_or(Error::QueueChanged)?;
        queue.current = Some(entry.clone());
        if queue.mode == PlaybackMode::Random {
            let inserted = if replace {
                queue
                    .items
                    .iter()
                    .map(|row| row.entry_id.as_str())
                    .collect()
            } else {
                HashSet::from([entry.as_str()])
            };
            queue.order = super::local_batch::random_order(
                &queue,
                &inserted,
                &entry,
                false,
                &mut rand::rng(),
            );
        }
        Ok((
            PlaybackCommand::PlayPodcastBrowse {
                update: QueueUpdate {
                    expected: before.fence(),
                    queue,
                    transport: Some(QueueTransport {
                        position: Some(position),
                        paused: false,
                    }),
                },
                first_source,
            },
            guard,
        ))
    }
}
