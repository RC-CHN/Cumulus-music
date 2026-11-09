//! Ordinary audio-detail program actions, separate from downloadedVoice.
use super::{MusicClient, PodcastLibrary, PodcastScope, SessionToken, identifier, json};
use crate::{
    Error, Result,
    client::dto::AccountData,
    comments::{CommentApi, ResourceKind},
    network_diagnostics::DefaultRoute,
};
use serde::Deserialize;
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PodcastProgramTarget {
    pub radio_id: String,
    pub ascending: bool,
    pub revision: String,
    pub voice_id: String,
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
        let value = if route == DefaultRoute::Missing {
            None
        } else {
            let id = target.voice_id.clone();
            let count = tokio::time::timeout(
                Duration::from_millis(1_500),
                self.session.run_for(&token, |api, _| async move {
                    CommentApi::new(api).count(ResourceKind::Voice, id).await
                }),
            )
            .await;
            self.require_podcast_program(&token, &target, false)?;
            count.ok().and_then(Result::ok)
        };
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
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
