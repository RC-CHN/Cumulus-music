//! M/404: initial play reads Week from the virtual playlist's target.
//! M/537: subsequent card clicks pause or resume the exact active source.
use super::{MusicClient, ProfilePlayRecordPeriod as Period, rank_rows, target, visible};
use crate::{
    Error, Result,
    client::{dto::PlayerView, focus::Owner, mapping, player},
    membership::Quality,
    playback::{
        controller::{PlaybackCommand, PlaybackStatus},
        engine::Cancellation,
        profile_rank,
    },
};
use serde_json::json;
use std::{collections::HashSet, sync::atomic::Ordering};

impl MusicClient {
    pub(in crate::client) async fn play_profile_rank_playlist(
        &self,
        epoch: &str,
        uid: Option<&str>,
        quality: Quality,
    ) -> Result<PlayerView> {
        let token = self.playlist_token(epoch)?;
        let (uid, is_self) = target(&self.session, &token, uid)?;
        let focus = self.focus.snapshot()?;
        let observed = self.player.selection.load(Ordering::Acquire);
        let controller = self.player.controller().await?;
        let before = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        let expected = before.fence();
        if before.fm || self.together_attached(&token, None)? {
            return Err(Error::State("listening rank requires ordinary playback"));
        }
        let same_source = before
            .selected
            .as_ref()
            .is_some_and(|entry| profile_rank::uid(&entry.source) == Some(uid.as_str()));
        let command = if same_source
            && matches!(
                before.state,
                PlaybackStatus::Playing | PlaybackStatus::Buffering | PlaybackStatus::Paused
            ) {
            PlaybackCommand::Pause(before.state != PlaybackStatus::Paused)
        } else {
            let resources = self.session.run_for(&token, |api, _| {
                let token = &token;
                let uid = &uid;
                async move {
                    if !is_self && !visible(&self.session, token, &api, uid).await? {
                        return Err(Error::State("listening rank playback is private"));
                    }
                    let rows = rank_rows(api.as_ref(), uid, Period::Week).await?
                        .ok_or(Error::State("listening rank playback is private"))?;
                    let mut seen = HashSet::new();
                    let mut resources = Vec::new();
                    for row in rows {
                        let song = &row["song"];
                        if !mapping::online_play_all_eligible(song) {
                            continue;
                        }
                        let mut view = mapping::track(song)?;
                        view.voice_id = None;
                        if seen.insert(view.id.clone()) {
                            resources.push(json!({"id":view.id,"dt":view.duration_ms,
                                "level":quality.as_str(),"song":mapping::projected_song_metadata(&view)}));
                        }
                    }
                    if resources.is_empty() {
                        return Err(Error::State("listening rank has no playable songs"));
                    }
                    Ok(resources)
                }
            }).await?;
            PlaybackCommand::ReplaceProfileRankPlaylist {
                resources,
                source: profile_rank::source(&uid, is_self),
            }
        };
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        if self.together_attached(&token, None)? {
            return Err(Error::State("listening rank requires ordinary playback"));
        }
        if controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?
            .fence()
            != expected
        {
            return Err(Error::StaleResponse);
        }
        let pause = matches!(command, PlaybackCommand::Pause(true));
        let (lease, attempt) = if pause {
            (None, observed)
        } else {
            let (lease, attempt) = self.reserve_prepared_audio(&token, focus, || {
                if self.player.selection.load(Ordering::Acquire) != observed {
                    return Err(Error::StaleResponse);
                }
                Ok(self.player.selection.fetch_add(1, Ordering::AcqRel) + 1)
            })?;
            self.finish_media_claim(&token, Owner::Audio, lease).await?;
            (Some(lease), attempt)
        };
        let selection = self.player.selection.clone();
        let native_focus = self.focus.clone();
        let ordinary = self.together_detached_guard(&token);
        let state = controller
            .command_guarded(
                &token,
                command,
                expected,
                Cancellation::new(move || {
                    selection.load(Ordering::Acquire) == attempt
                        && !ordinary.is_cancelled()
                        && lease.map_or_else(
                            || native_focus.check_snapshot(focus).is_ok(),
                            |lease| native_focus.check(lease, Owner::Audio).is_ok(),
                        )
                }),
            )
            .await?;
        self.session.check_generation(&token)?;
        player::project(&state, &token)
    }
}
