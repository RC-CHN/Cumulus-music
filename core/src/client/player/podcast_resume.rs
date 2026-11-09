//! M622 miniature-bar resume setting, bound to the current native voice.
use super::{MusicClient, PlaybackSnapshot, PlaybackStatus, voice_detail, voice_scope};
use crate::{
    Error, Result,
    account::SessionToken,
    client::dto::AccountData,
    podcast::{PodcastAction, PodcastService, resume},
    protocol::api::{Api, PlayerPodcastResumeWrite},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlayerPodcastResumeAction {
    View,
    Change { enabled: bool },
    Reconcile,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerPodcastResumeView {
    radio_id: String,
    enabled: bool,
    pending: bool,
}

#[derive(PartialEq, Eq)]
struct CurrentPodcast {
    voice: voice_scope::VoicePlaybackSeed,
    radio: String,
}

fn current(
    snapshot: &PlaybackSnapshot,
    token: &SessionToken,
    play_id: &str,
    entry_id: &str,
) -> Result<CurrentPodcast> {
    if !matches!(
        snapshot.state,
        PlaybackStatus::Playing | PlaybackStatus::Paused
    ) {
        return Err(Error::StaleResponse);
    }
    let voice = voice_scope::seed(snapshot, token, play_id)?;
    let entry = snapshot.selected.as_ref().ok_or(Error::StaleResponse)?;
    let (_, _, raw) = voice_detail::bound_episode(&entry.resource).ok_or(Error::StaleResponse)?;
    let radio = voice_detail::radio_id(raw).ok_or(Error::StaleResponse)?;
    if voice.entry_id != entry_id || PlayerPodcastResumeWrite::new(&radio, false).is_err() {
        return Err(Error::StaleResponse);
    }
    Ok(CurrentPodcast { voice, radio })
}

impl MusicClient {
    fn player_resume_group(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        revision: &str,
        viewing: bool,
    ) -> Result<bool> {
        // This is a passive check of the native live assignment. The menu
        // never starts a separate assignment request and cannot extend its TTL.
        let group = if viewing {
            // Official display retains the previous confirmed same-scope value
            // after a failed refresh; it never renews dispatch permission.
            self.pc_experiments
                .presented_group(token, api, revision, resume::EXPERIMENT)
        } else {
            self.pc_experiments
                .observed_group(token, api, revision, resume::EXPERIMENT)?
        };
        Ok(group.as_deref() == Some("t1"))
    }

    pub async fn player_podcast_resume(
        &self,
        epoch: &str,
        play_id: &str,
        entry_id: &str,
        action: PlayerPodcastResumeAction,
    ) -> Result<AccountData<Option<PlayerPodcastResumeView>>> {
        let token = self.player_token(epoch)?;
        let hidden = || AccountData {
            epoch: epoch.into(),
            value: None,
        };
        let viewing = matches!(action, PlayerPodcastResumeAction::View);
        if token.uid.is_none() || (!viewing && self.login_pending()?) {
            return if viewing {
                Ok(hidden())
            } else {
                Err(Error::StaleResponse)
            };
        }
        let api = self.session.native_api(&token)?;
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let Ok(podcast) = current(&controller.snapshot(), &token, play_id, entry_id) else {
            return if viewing {
                Ok(hidden())
            } else {
                Err(Error::StaleResponse)
            };
        };
        let room = self.together_detached_guard(&token);
        let revision = api.player_podcast_resume_write_revision().ok().flatten();
        let Some(revision) = revision else {
            return if viewing {
                Ok(hidden())
            } else {
                Err(Error::StaleResponse)
            };
        };
        let check = || {
            self.session
                .with_verified_profile(&token, &api, |_, _| Ok(()))?;
            if (!viewing && self.login_pending()?)
                || room.is_cancelled()
                || current(&controller.snapshot(), &token, play_id, entry_id)? != podcast
                || api.player_podcast_resume_write_revision()?.as_deref() != Some(revision.as_str())
                || !self.player_resume_group(&token, &api, &revision, viewing)?
            {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        if let Err(error) = check() {
            return if viewing { Ok(hidden()) } else { Err(error) };
        }
        let service = PodcastService::new(self.session.clone())?;
        match action {
            PlayerPodcastResumeAction::View => {}
            PlayerPodcastResumeAction::Change { enabled } => {
                service
                    .change_player_resume_for(&token, &podcast.radio, enabled, &revision, &check)
                    .await?;
            }
            PlayerPodcastResumeAction::Reconcile => {
                service
                    .reconcile_player_resume_for(
                        &token,
                        &podcast.radio,
                        resume::PlayerBinding {
                            revision: &revision,
                            permission: &check,
                        },
                    )
                    .await?;
            }
        }
        let pending = service
            .operation_for(&token, PodcastAction::Resume, &podcast.radio)?
            .is_some_and(|operation| operation.phase.pending());
        let enabled = self
            .session
            .podcast_resume
            .known(&token, &api, &revision, &podcast.radio)?
            .unwrap_or(true);
        check()?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: Some(PlayerPodcastResumeView {
                radio_id: podcast.radio,
                enabled,
                pending,
            }),
        })
    }
}
