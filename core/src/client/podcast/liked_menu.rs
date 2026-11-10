//! Native authority for normal programme rows in /myAudio/LikedVoiceTab.
use super::{
    EpisodeView, MusicClient, PodcastLibrary, PodcastRecentProgramMenu, PodcastScope, SessionToken,
    view,
};
use crate::{
    Error, Result,
    account::AccountSession,
    client::dto::{AccountData, SongDownloadState},
    network_diagnostics::DefaultRoute,
    podcast::Episode,
    protocol::api::identifier,
    storage::OperationLock,
};
use serde::Deserialize;
use serde_json::json;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PodcastLikedProgramTarget {
    pub revision: String,
    pub voice_id: String,
}

pub(in crate::client) fn liked_program(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    target: &PodcastLikedProgramTarget,
) -> Result<Episode> {
    let id = identifier(&json!(target.voice_id))?;
    let snapshot = PodcastLibrary::new(session.clone())?
        .cached_for(token, &PodcastScope::Liked)?
        .filter(|snapshot| snapshot.revision == target.revision)
        .ok_or(Error::QueueChanged)?;
    let mut rows = snapshot.voices.into_iter().filter(|row| row.id == id);
    let row = rows.next().ok_or(Error::QueueChanged)?;
    if rows.next().is_some() || row.display_status_withdrawn() {
        return Err(Error::QueueChanged);
    }
    Ok(row)
}

pub(in crate::client) fn liked_playable_program(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    target: &PodcastLikedProgramTarget,
) -> Result<Episode> {
    let row = liked_program(session, token, target)?;
    if row.raw["grayDisplay"] == true {
        return Err(Error::QueueChanged);
    }
    Ok(row)
}

pub(in crate::client) fn lock_liked_download_voice(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    target: &PodcastLikedProgramTarget,
) -> Result<OperationLock> {
    let lock = PodcastLibrary::new(session.clone())?.lock_liked_for(token)?;
    liked_playable_program(session, token, target)?;
    Ok(lock)
}

impl MusicClient {
    pub fn podcast_liked_program(
        &self,
        epoch: &str,
        target: &PodcastLikedProgramTarget,
    ) -> Result<AccountData<EpisodeView>> {
        let token = self.podcast_token(epoch)?;
        let row = liked_program(&self.session, &token, target)?;
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: view::episode(&row, std::slice::from_ref(&row.id)),
        })
    }

    pub async fn podcast_liked_program_menu(
        &self,
        epoch: &str,
        target: &PodcastLikedProgramTarget,
        route: DefaultRoute,
    ) -> Result<AccountData<PodcastRecentProgramMenu>> {
        let token = self.podcast_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let current = || {
            self.session.require_api(&token, &api)?;
            liked_program(&self.session, &token, target)
        };
        let row = current()?;
        let comment_count = self
            .read_program_comment_count(&token, &row.id, route)
            .await;
        current()?;
        let download_complete = self
            .voice_download_state(epoch, &row.id)
            .await
            .ok()
            .map(|status| status == SongDownloadState::Complete);
        current()?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: PodcastRecentProgramMenu {
                comment_count,
                download_complete,
            },
        })
    }

    pub async fn podcast_liked_program_directory_path(
        &self,
        epoch: &str,
        target: &PodcastLikedProgramTarget,
    ) -> Result<PathBuf> {
        let token = self.podcast_token(epoch)?;
        let row = liked_program(&self.session, &token, target)?;
        let path = self.completed_voice_directory(epoch, &row.id).await?;
        liked_program(&self.session, &token, target)?;
        Ok(path)
    }
}
