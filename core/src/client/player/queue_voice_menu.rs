//! The ordinary queue uses the track menu template with an episode wrapper (M556).
use super::{MusicClient, PlayerResourceKind, QueueVoiceMenu, identity, track, voice_detail};
use crate::{
    Error, Result, account::SessionToken, client::dto::SongDownloadState,
    network_diagnostics::DefaultRoute, playback::QueueEntry,
};
use serde_json::Value;
use std::path::PathBuf;

pub(super) fn eligible(resource: &Value) -> bool {
    identity::kind(resource) == PlayerResourceKind::Voice
        && voice_detail::bound_episode(resource).is_some()
}

impl MusicClient {
    async fn voice_menu_entry(
        &self,
        token: &SessionToken,
        revision: u64,
        entry_id: &str,
    ) -> Result<QueueEntry> {
        if token.uid.is_none() || self.login_pending()? {
            return Err(Error::StaleResponse);
        }
        let (state, queue) = self
            .player
            .controller()
            .await?
            .queue_with_state(token)
            .await?;
        if state.queue_revision != revision {
            return Err(Error::QueueChanged);
        }
        if state.fm
            || queue.intelligence.is_some()
            || self.queue_clear_context(token)?.room_id.is_some()
        {
            return Err(Error::State("voice menu requires the ordinary queue"));
        }
        queue
            .items
            .into_iter()
            .find(|entry| {
                entry.entry_id == entry_id && entry.visible_in_queue() && eligible(&entry.resource)
            })
            .ok_or(Error::InvalidInput(
                "queue menu requires a visible online episode",
            ))
    }

    pub async fn player_queue_voice_menu(
        &self,
        epoch: &str,
        revision: &str,
        entry_id: &str,
        route: DefaultRoute,
    ) -> Result<QueueVoiceMenu> {
        let token = self.player_token(epoch)?;
        let version = revision
            .parse()
            .map_err(|_| Error::InvalidInput("invalid queue menu revision"))?;
        let entry = self.voice_menu_entry(&token, version, entry_id).await?;
        let (voice_id, _, voice) = voice_detail::bound_episode(&entry.resource)
            .ok_or(Error::InvalidInput("invalid queue episode identity"))?;
        let api = self.session.native_api(&token)?;
        let credentials = api.search_privilege_credential_revision()?;
        let comment_count = self
            .read_program_comment_count(&token, &voice_id, route)
            .await;
        let downloaded = self
            .voice_download_state(epoch, &voice_id)
            .await
            .ok()
            .map(|state| state == SongDownloadState::Complete);
        let directory_available = downloaded == Some(true)
            && self
                .completed_voice_directory(epoch, &voice_id)
                .await
                .is_ok();
        if self.voice_menu_entry(&token, version, entry_id).await? != entry
            || api.search_privilege_credential_revision()? != credentials
        {
            return Err(Error::QueueChanged);
        }
        self.session.require_api(&token, &api)?;
        // M556 replaces the stored source label and destination with the
        // episode's own audio when its nonempty name differs. Cumulus only
        // exposes a typed radio destination when native identity agrees.
        let radio = voice_detail::radio(voice, token.uid.as_deref());
        let fallback = if entry.source["scene"] == "search" {
            "搜索"
        } else {
            "未知"
        };
        Ok(QueueVoiceMenu {
            epoch: epoch.into(),
            revision: revision.into(),
            entry_id: entry_id.into(),
            track: track(&entry.resource)?,
            voice_id,
            source_label: radio
                .as_ref()
                .map(|radio| radio.name.as_str())
                .unwrap_or(fallback)
                .into(),
            radio_id: radio.map(|radio| radio.id),
            creator: voice_detail::creator(voice),
            comment_count,
            downloaded,
            directory_available,
        })
    }

    /// Keep the resolved directory within the native desktop boundary.
    pub async fn player_queue_voice_directory(
        &self,
        epoch: &str,
        revision: &str,
        entry_id: &str,
    ) -> Result<PathBuf> {
        let token = self.player_token(epoch)?;
        let version = revision
            .parse()
            .map_err(|_| Error::InvalidInput("invalid queue menu revision"))?;
        let entry = self.voice_menu_entry(&token, version, entry_id).await?;
        let (id, _, _) = voice_detail::bound_episode(&entry.resource)
            .ok_or(Error::InvalidInput("invalid queue episode identity"))?;
        let path = self.completed_voice_directory(epoch, &id).await?;
        if self.voice_menu_entry(&token, version, entry_id).await? != entry {
            return Err(Error::QueueChanged);
        }
        self.session.check_generation(&token)?;
        Ok(path)
    }
}
