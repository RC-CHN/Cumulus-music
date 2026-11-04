//! Explicit replacement of one failed download queue entry, after reauthorization.
use super::{MusicClient, PlayerView, mapping, project, source_control};
use crate::{
    Error, Result,
    account::SessionStatus,
    membership::Quality,
    playback::{
        controller::{PlaybackCommand, PlaybackStatus},
        engine::Cancellation,
        failure::PlaybackFailure,
    },
    storage::OperationLock,
};
use serde_json::json;
use std::sync::atomic::Ordering;

impl MusicClient {
    pub(super) async fn play_missing_download_online(
        &self,
        epoch: &str,
        play_id: &str,
        revision: &str,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        let account = self.session.snapshot()?;
        if account.token != token || account.status != SessionStatus::Authenticated {
            return Err(Error::StaleResponse);
        }
        let selection = self.player.selection.clone();
        let observed = selection.load(Ordering::Acquire);
        let controller = self.player.controller().await?;
        let current = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        source_control::validate_identity(&current, play_id, revision)?;
        if current.state != PlaybackStatus::Error
            || current.fm
            || current.intelligence.is_some()
            || !matches!(current.error, Some(PlaybackFailure::DownloadedFileMissing))
        {
            return Err(Error::StaleResponse);
        }
        let entry = current.selected.as_ref().ok_or(Error::StaleResponse)?;
        let focus = self.focus.clone();
        let owner = focus.snapshot()?;
        let room = self.together_detached_guard(&token);
        if owner.owner != Some(super::super::focus::Owner::Audio) || room.is_cancelled() {
            return Err(Error::StaleResponse);
        }
        // No duplicate authorization for the same failed source while an
        // explicit attempt is outstanding. This is not the task/file lock.
        let _operation = OperationLock::acquire(
            self.player.store.directory(),
            "missing-download-online",
            &serde_json::to_string(&(&token.uid, &entry.entry_id))?,
        )?;
        let resolver = controller.source_resolver();
        let candidate = resolver
            .missing_catalog_download(&entry.resource, &token)
            .await?
            .ok_or(Error::StaleResponse)?;
        let quality = if current.quality_preferences.damaged {
            Quality::Standard
        } else {
            current.quality_preferences.preferences.quality
        };
        let preference_revision = current.quality_preferences.preferences.revision.clone();
        let expected = current.fence();
        let confirmed = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        if confirmed.fence() != expected
            || confirmed.quality_preferences.preferences.revision != preference_revision
            || room.is_cancelled()
        {
            return Err(Error::StaleResponse);
        }
        focus.check_snapshot(owner)?;
        let attempt = observed
            .checked_add(1)
            .ok_or(Error::State("playback selection exhausted"))?;
        selection
            .compare_exchange(observed, attempt, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::StaleResponse)?;
        let authority = Cancellation::new(move || {
            selection.load(Ordering::Acquire) == attempt
                && focus.check_snapshot(owner).is_ok()
                && !room.is_cancelled()
        });
        let track = mapping::track(&candidate.song)?;
        if entry.resource["id"].as_str() != Some(track.id.as_str()) {
            return Err(Error::StaleResponse);
        }
        // Never carry the downloaded path, task, offline rights, or local
        // clock into this fresh normal online authorization.
        let resource = json!({
            "id":track.id,
            "dt":(track.duration_ms > 0).then_some(track.duration_ms),
            "song":mapping::projected_song_metadata(&track),
            "level":quality.as_str()
        });
        authority.check()?;
        let source = resolver.resolve_fresh_online(&resource, &token).await?;
        authority.check()?;
        if resolver
            .missing_catalog_download(&entry.resource, &token)
            .await?
            .as_ref()
            != Some(&candidate)
        {
            return Err(Error::StaleResponse);
        }
        let state = controller
            .command_guarded(
                &token,
                PlaybackCommand::UseMissingDownloadOnline {
                    resource,
                    source,
                    preference_revision,
                },
                expected,
                authority,
            )
            .await?;
        self.session.check_generation(&token)?;
        project(&state, &token)
    }
}
