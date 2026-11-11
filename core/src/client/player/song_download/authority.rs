//! Proof of the current source independent of renderer IDs and menu state.
use super::{CurrentSongDownloadTarget, MusicClient, SongRights};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    catalog::CatalogApi,
    client::{
        downloads::purchased::{purchased_quality_guide_url, vivid_available},
        focus::{FocusSnapshot, MediaFocus, Owner},
        pc_experiments::PcExperimentHost,
        together::RoomDetachment,
    },
    membership::Quality,
    playback::{
        TimelineMode,
        controller::{PlaybackController, PlaybackSnapshot, PlaybackStatus},
    },
    protocol::api::{Api, identifier},
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use crate::client::player::song_purchase::{Candidate, Observation, ordinary_entry};
use serde_json::json;

pub(super) struct Authority {
    pub(super) token: SessionToken,
    pub(super) song: String,
    pub(super) rights: SongRights,
    pub(super) collectable: bool,
    pub(super) target: CurrentSongDownloadTarget,
    proof: Observation,
    session: Arc<AccountSession>,
    api: Arc<dyn Api>,
    experiments: Arc<PcExperimentHost>,
    controller: Arc<PlaybackController>,
    selection: Arc<AtomicU64>,
    focus: Arc<MediaFocus>,
    expected_focus: FocusSnapshot,
    detached: RoomDetachment,
    no_login: Box<dyn Fn() -> Result<()> + Send + Sync>,
}

#[derive(Clone, Copy)]
pub(super) enum CredentialScope {
    Download,
    Picker,
    Create,
}

fn matches(
    state: &PlaybackSnapshot,
    token: &SessionToken,
    target: &CurrentSongDownloadTarget,
) -> bool {
    &state.account == token
        && state.play_id.as_deref() == Some(target.play_id.as_str())
        && state.selected.as_ref().map(|entry| entry.entry_id.as_str())
            == Some(target.entry_id.as_str())
}

fn eligible(state: &PlaybackSnapshot, token: &SessionToken, proof: &Observation) -> bool {
    matches!(
        state.state,
        PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
    ) && state
        .timeline
        .as_ref()
        .is_some_and(|timeline| timeline.mode == TimelineMode::Full)
        && ordinary_entry(state, token, &proof.song_id) == Some(proof.entry_id.as_str())
}

impl Authority {
    /// Works while AccountSession/SQLite are already locked. No account call,
    /// credential snapshot, HTTP, or database access inside this guard.
    pub(super) fn playback(&self) -> Result<()> {
        self.focus.check_snapshot(self.expected_focus)?;
        if !self.detached.is_detached(&self.token)?
            || self.selection.load(Ordering::Acquire) != self.proof.selection
        {
            return Err(Error::StaleResponse);
        }
        let state = self.controller.snapshot();
        if !matches(&state, &self.token, &self.target)
            || !eligible(&state, &self.token, &self.proof)
            || self.selection.load(Ordering::Acquire) != self.proof.selection
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    pub(super) fn checked(&self, scope: CredentialScope) -> Result<Vec<Quality>> {
        self.checked_rights(scope).map(|(qualities, _)| qualities)
    }

    pub(super) fn guide(&self) -> Result<&'static str> {
        let (qualities, vivid) = self.checked_rights(CredentialScope::Download)?;
        if qualities.is_empty() {
            return Err(Error::StaleResponse);
        }
        Ok(purchased_quality_guide_url(vivid))
    }

    fn checked_rights(&self, scope: CredentialScope) -> Result<(Vec<Quality>, bool)> {
        (self.no_login)()?;
        self.playback()?;
        let qualities = self
            .session
            .with_verified_profile(&self.token, &self.api, |_, _| {
                let revision = &self.proof.revision;
                let exact = |current: Result<Option<String>>| {
                    if current.map_err(|_| Error::StaleResponse)?.as_ref() != Some(revision) {
                        return Err(Error::StaleResponse);
                    }
                    Ok(())
                };
                exact(self.api.player_song_purchase_credential_revision())?;
                if matches!(scope, CredentialScope::Picker | CredentialScope::Create) {
                    exact(self.api.player_song_download_picker_credential_revision())?;
                    exact(self.api.song_card_playlist_scope_revision())?;
                }
                if matches!(scope, CredentialScope::Create) {
                    exact(self.api.song_card_playlist_create_scope_revision())?;
                }
                match self.rights {
                    SongRights::Ordinary => Ok((Vec::new(), false)),
                    SongRights::Purchased(offer) => {
                        let groups =
                            self.experiments
                                .observed_groups(&self.token, &self.api, revision)?;
                        let vivid = vivid_available(groups.as_ref());
                        Ok((offer.qualities(vivid), vivid))
                    }
                }
            })?;
        (self.no_login)()?;
        self.playback()?;
        Ok(qualities)
    }
}

impl MusicClient {
    /// Automatic Next/cold restore does not create a Phase322 observation.
    /// An explicit current-song download may use one exact native read for
    /// the still-loaded catalogue row; no account proof means no request.
    pub(super) async fn observe_current_song_download(
        &self,
        epoch: &str,
        target: &CurrentSongDownloadTarget,
    ) -> Result<()> {
        self.auth.pending_guard()()?;
        let token = self.player_token(epoch)?;
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let generation = self.player.selection.load(Ordering::Acquire);
        let initial = controller.snapshot();
        if !matches(&initial, &token, target) {
            return Err(Error::StaleResponse);
        }
        let Some(id) = initial
            .selected
            .as_ref()
            .and_then(|entry| identifier(&entry.resource["song"]["id"]).ok())
        else {
            return Ok(());
        };
        let focus = self.focus.snapshot()?;
        let eligible = |state: &PlaybackSnapshot| {
            matches!(
                state.state,
                PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
            ) && state
                .timeline
                .as_ref()
                .is_some_and(|timeline| timeline.mode == TimelineMode::Full)
                && ordinary_entry(state, &token, &id) == Some(target.entry_id.as_str())
        };
        if focus.owner != Some(Owner::Audio)
            || self.together_attached(&token, None)?
            || !eligible(&initial)
        {
            return Ok(());
        }
        let check = |state: &PlaybackSnapshot| {
            self.auth.pending_guard()()?;
            if self.focus.snapshot()? != focus
                || self.together_attached(&token, None)?
                || self.player.selection.load(Ordering::Acquire) != generation
                || !matches(state, &token, target)
                || !eligible(state)
            {
                return Err(Error::StaleResponse);
            }
            self.session.check_generation(&token)
        };
        check(&initial)?;
        let candidate = self
            .session
            .run_for(&token, |api, _| async {
                if api.player_song_purchase_credential_revision()?.is_none() {
                    return Ok(None);
                }
                let (_, detail) = CatalogApi::new(api.clone())
                    .songs_for_current_purchase(&[json!(&id)], &id)
                    .await?;
                Ok(detail.and_then(|detail| Candidate::from_detail(&id, api, detail)))
            })
            .await?;
        let after = controller.snapshot();
        check(&after)?;
        if let Some(candidate) = candidate {
            self.player.song_purchase.observe(
                &self.player.selection,
                &after,
                &token,
                generation,
                candidate,
            );
        }
        Ok(())
    }

    pub(super) fn bind_current_song_download(
        &self,
        epoch: &str,
        target: &CurrentSongDownloadTarget,
    ) -> Result<Option<Authority>> {
        self.auth.pending_guard()()?;
        let token = self.player_token(epoch)?;
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let selection = self.player.selection.load(Ordering::Acquire);
        let state = controller.snapshot();
        if !matches(&state, &token, target) {
            return Err(Error::StaleResponse);
        }
        let Some(proof) = self.player.song_purchase.current(
            &token,
            selection,
            &target.play_id,
            &target.entry_id,
        )?
        else {
            return Ok(None);
        };
        let focus = self.focus.snapshot()?;
        if !eligible(&state, &token, &proof)
            || focus.owner != Some(Owner::Audio)
            || self.together_attached(&token, None)?
        {
            return Ok(None);
        }
        let api = proof.api.upgrade().ok_or(Error::StaleResponse)?;
        let authority = Authority {
            token,
            song: proof.song_id.clone(),
            rights: proof.rights,
            collectable: proof.picker_collectable,
            target: target.clone(),
            proof,
            session: self.session.clone(),
            api,
            experiments: self.pc_experiments.clone(),
            controller: controller.clone(),
            selection: self.player.selection.clone(),
            focus: self.focus.clone(),
            expected_focus: focus,
            detached: self.together.detachment(),
            no_login: Box::new(self.auth.pending_guard()),
        };
        if let Err(error) = authority.checked(CredentialScope::Download) {
            if matches!(error, Error::StaleResponse) {
                self.player.song_purchase.retire(&authority.proof);
            }
            return Err(error);
        }
        Ok(Some(authority))
    }
}
