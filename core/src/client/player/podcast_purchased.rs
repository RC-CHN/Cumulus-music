//! Single-program actions in the mixed purchase and liked tables.
use super::{
    MusicClient, PlaybackCommand, PlaybackStatus, PlayerResourceKind, identity,
    podcast_browse::resource, prepared_selection::PreparedSelection,
    queue_playlist::insert_collection_bound, voice_detail::bound_episode,
};
use crate::{
    Error, Result,
    account::SessionToken,
    client::podcast::{
        PodcastLikedProgramTarget, PodcastPurchasedTarget, liked_menu::liked_playable_program,
        purchased::purchased_playback_episode,
    },
    playback::{
        PlaybackMode,
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
}
impl ScopedProgram<'_> {
    pub(super) fn origin(&self) -> Value {
        match self {
            Self::Purchased(_) => purchased_origin(),
            Self::Liked(target) => {
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

fn same_online_voice(value: &Value, episode: &Episode) -> bool {
    identity::kind(value) == PlayerResourceKind::Voice
        && bound_episode(value).is_some_and(|(id, song, voice)| {
            id == episode.id
                && episode.song_id.as_ref() == Some(&song)
                && episode
                    .radio
                    .as_ref()
                    .is_none_or(|radio| voice["audio"]["id"] == radio.id)
        })
}

impl MusicClient {
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

    pub(super) async fn scoped_program_selection(
        &self,
        token: &SessionToken,
        target: ScopedProgram<'_>,
        menu: bool,
        prepared: &PreparedSelection,
    ) -> Result<(PlaybackCommand, Cancellation)> {
        let (cached, api, guard) = match target {
            ScopedProgram::Purchased(target) => self.purchased_episode_scope(token, target)?,
            ScopedProgram::Liked(target) => self.liked_episode_scope(token, target)?,
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
        let loaded = |episode: &Episode| {
            before.play_id.is_some()
                && matches!(
                    before.state,
                    PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
                )
                && before
                    .selected
                    .as_ref()
                    .is_some_and(|entry| same_online_voice(&entry.resource, episode))
        };
        let current_command = || {
            if !menu {
                PlaybackCommand::Pause(!before.paused)
            } else if before.paused {
                PlaybackCommand::Pause(false)
            } else {
                PlaybackCommand::Snapshot
            }
        };
        if identifier(&cached.raw["mainSong"]["id"]).is_ok() && loaded(&cached) {
            return Ok((current_command(), guard));
        }

        let detail = if matches!(target, ScopedProgram::Purchased(_))
            || identifier(&cached.raw["mainSong"]["id"]).is_err()
        {
            self.read_scoped_program_detail(token, &cached, &preparing)
                .await?
        } else {
            cached
        };
        if loaded(&detail) {
            return Ok((current_command(), guard));
        }

        let selected = resource(&detail)?;
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
        queue = insert_collection_bound(
            queue,
            vec![selected.clone()],
            source.clone(),
            false,
            true,
            true,
        );
        let entry = queue
            .items
            .iter()
            .find(|entry| entry.resource == selected && entry.source == source)
            .map(|entry| entry.entry_id.clone())
            .ok_or(Error::QueueChanged)?;
        queue.current = Some(entry.clone());
        if queue.mode == PlaybackMode::Random {
            queue.order = super::local_batch::random_order(
                &queue,
                &HashSet::from([entry.as_str()]),
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
