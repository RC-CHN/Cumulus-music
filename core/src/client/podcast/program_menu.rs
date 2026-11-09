//! Ordinary program row actions, separate from downloadedVoice.
use super::{
    EpisodeView, MusicClient, PodcastLibrary, PodcastRecentProgramTarget, PodcastScope,
    SessionToken, identifier, json,
};
use crate::{
    Error, Result,
    client::dto::{AccountData, SongDownloadState},
    comments::{CommentApi, ResourceKind},
    network_diagnostics::DefaultRoute,
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PodcastProgramTarget {
    pub radio_id: String,
    pub ascending: bool,
    pub revision: String,
    pub voice_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastRecentProgramMenu {
    comment_count: Option<u64>,
    download_complete: Option<bool>,
}

impl MusicClient {
    pub(in crate::client) fn require_podcast_program(
        &self,
        token: &SessionToken,
        target: &PodcastProgramTarget,
        playable: bool,
    ) -> Result<()> {
        let scope = PodcastScope::Radio {
            id: identifier(&json!(target.radio_id))?,
            ascending: target.ascending,
        };
        let voice = identifier(&json!(target.voice_id))?;
        let library = PodcastLibrary::new(self.session.clone())?;
        if library
            .radio_order_for(token, &target.radio_id)?
            .is_some_and(|order| order != target.ascending)
        {
            return Err(Error::QueueChanged);
        }
        let snapshot = library
            .cached_for(token, &scope)?
            .filter(|snapshot| {
                snapshot.revision == target.revision
                    && snapshot
                        .radios
                        .first()
                        .is_some_and(|radio| radio.id == target.radio_id)
            })
            .ok_or(Error::QueueChanged)?;
        let mut matches = snapshot.voices.iter().filter(|episode| episode.id == voice);
        let episode = matches.next().ok_or(Error::QueueChanged)?;
        if matches.next().is_some() || (playable && episode.raw["grayDisplay"] == true) {
            return Err(Error::QueueChanged);
        }
        Ok(())
    }

    pub async fn podcast_program_comment_count(
        &self,
        epoch: &str,
        target: PodcastProgramTarget,
        route: DefaultRoute,
    ) -> Result<AccountData<Option<u64>>> {
        let token = self.podcast_token(epoch)?;
        self.require_podcast_program(&token, &target, false)?;
        let value = self
            .read_program_comment_count(&token, &target.voice_id, route)
            .await;
        if route != DefaultRoute::Missing {
            self.require_podcast_program(&token, &target, false)?;
        }
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    async fn read_program_comment_count(
        &self,
        token: &SessionToken,
        id: &str,
        route: DefaultRoute,
    ) -> Option<u64> {
        if route == DefaultRoute::Missing {
            return None;
        }
        let id = id.to_owned();
        tokio::time::timeout(
            Duration::from_millis(1_500),
            self.session.run_for(token, |api, _| async move {
                CommentApi::new(api).count(ResourceKind::Voice, id).await
            }),
        )
        .await
        .ok()
        .and_then(Result::ok)
    }

    pub async fn podcast_recent_program_menu(
        &self,
        epoch: &str,
        target: PodcastRecentProgramTarget,
        route: DefaultRoute,
    ) -> Result<AccountData<PodcastRecentProgramMenu>> {
        let token = self.podcast_token(epoch)?;
        let (row, source) = self.recent_program_for(&token, &target, false)?;
        let still_current = || {
            self.session.check_generation(&token)?;
            if source.is_cancelled() {
                return Err(Error::QueueChanged);
            }
            Ok(())
        };
        let comment_count = self
            .read_program_comment_count(&token, &row.id, route)
            .await;
        still_current()?;
        let download_complete = self
            .voice_download_state(epoch, &row.id)
            .await
            .ok()
            .map(|status| status == SongDownloadState::Complete);
        still_current()?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: PodcastRecentProgramMenu {
                comment_count,
                download_complete,
            },
        })
    }

    /// Resolve link and download targets from the same current native row.
    pub fn podcast_recent_program(
        &self,
        epoch: &str,
        target: PodcastRecentProgramTarget,
    ) -> Result<AccountData<EpisodeView>> {
        let token = self.podcast_token(epoch)?;
        let (row, source) = self.recent_program_for(&token, &target, false)?;
        let value = Self::recent_program_view(&row);
        self.session.check_generation(&token)?;
        if source.is_cancelled() {
            return Err(Error::QueueChanged);
        }
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    pub async fn podcast_recent_program_directory_path(
        &self,
        epoch: &str,
        target: PodcastRecentProgramTarget,
    ) -> Result<PathBuf> {
        let token = self.podcast_token(epoch)?;
        let (row, source) = self.recent_program_for(&token, &target, false)?;
        let path = self.completed_voice_directory(epoch, &row.id).await?;
        self.session.check_generation(&token)?;
        if source.is_cancelled() {
            return Err(Error::QueueChanged);
        }
        Ok(path)
    }

    /// Native shell only. No selected or persisted path crosses into the WebView.
    pub async fn podcast_program_directory_path(
        &self,
        epoch: &str,
        target: PodcastProgramTarget,
    ) -> Result<PathBuf> {
        let token = self.podcast_token(epoch)?;
        self.require_podcast_program(&token, &target, false)?;
        let path = self
            .completed_voice_directory(epoch, &target.voice_id)
            .await?;
        self.require_podcast_program(&token, &target, false)?;
        Ok(path)
    }
}
