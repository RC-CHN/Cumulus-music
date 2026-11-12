//! Settings defaults are the local M/116 `updateQuantity` operation, not the
//! playback bar's current-source authorization and switch.
use super::MusicClient;
use crate::{
    Error, Result,
    account::SessionToken,
    client::downloads::{
        QualityAccess, QualityChoice, purchased::vivid_available, quality_choices,
    },
    membership::Quality,
    playback::{
        quality::trials::QualityTrials,
        quality_preferences::{QualityPreferences, QualityPreferencesSnapshot},
    },
    storage::StateTransaction,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackQualityPreferencesView {
    pub epoch: String,
    /// Null when the local record is damaged or a trial prevents changing it.
    pub revision: Option<String>,
    pub quality: Quality,
    pub damaged: bool,
    choices: Vec<QualityChoice>,
}

fn revision(
    epoch: &str,
    preference: &QualityPreferences,
    choices: &[QualityChoice],
) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(
        epoch,
        &preference.revision,
        choices,
    ))?)))
}

fn blocked(
    tx: &StateTransaction<'_>,
    owner: &str,
    token: &SessionToken,
    preference: &QualityPreferences,
) -> Result<bool> {
    Ok(preference.trial.is_some()
        || (token.uid.is_some() && QualityTrials::default_preference_blocked_in(tx, owner)?))
}

impl MusicClient {
    fn playback_default_choices(&self, token: &SessionToken) -> Result<Vec<QualityChoice>> {
        if token.uid.is_some() {
            return self.download_quality_choices(token);
        }
        let api = self.session.native_api(token)?;
        let vivid = if let Ok(Some(credential)) = api.verified_pc_experiment_revision() {
            let groups = self
                .pc_experiments
                .observed_groups(token, &api, &credential)?;
            vivid_available(groups.as_ref())
        } else {
            false
        };
        // M/3097 sends guests to login above ExHigh. These rows are present
        // but never selectable as a local guest, regardless of trial state.
        Ok(quality_choices(None, None, vivid))
    }

    pub async fn playback_quality_preferences(
        &self,
        epoch: &str,
        refresh: bool,
    ) -> Result<PlaybackQualityPreferencesView> {
        let token = self.player_token(epoch)?;
        if refresh && token.uid.is_some() {
            self.refresh_quality_membership(&token).await?;
        }
        let mut membership = self.session.subscribe_membership()?;
        drop(membership.borrow_and_update());
        let choices = self.playback_default_choices(&token)?;
        self.session.with_local_owner(&token, |owner| {
            if membership.has_changed().unwrap_or(true) {
                return Err(Error::State("listening quality settings changed"));
            }
            self.player.store.read_snapshot(|tx| {
                let current = QualityPreferencesSnapshot::read_in(tx, owner)?;
                let revision =
                    if current.damaged || blocked(tx, owner, &token, &current.preferences)? {
                        None
                    } else {
                        Some(revision(epoch, &current.preferences, &choices)?)
                    };
                Ok(PlaybackQualityPreferencesView {
                    epoch: epoch.into(),
                    revision,
                    quality: current.preferences.quality,
                    damaged: current.damaged,
                    choices,
                })
            })
        })
    }

    pub fn save_playback_quality_preference(
        &self,
        epoch: &str,
        expected: &str,
        quality: Quality,
    ) -> Result<PlaybackQualityPreferencesView> {
        let token = self.player_token(epoch)?;
        let _trial_lock = if let Some(owner) = &token.uid {
            Some(QualityTrials::new(self.session.clone())?.lock(owner)?)
        } else {
            None
        };
        let mut membership = self.session.subscribe_membership()?;
        drop(membership.borrow_and_update());
        let choices = self.playback_default_choices(&token)?;
        self.session.with_local_owner(&token, |owner| {
            // The account lock serializes verified membership commits with this
            // local CAS. No session/store lock or trial lane spans network I/O.
            if membership.has_changed().unwrap_or(true) {
                return Err(Error::State("listening quality settings changed"));
            }
            self.player.store.transaction(|tx| {
                let current = QualityPreferencesSnapshot::read_in(tx, owner)?;
                if current.damaged {
                    return Err(Error::State("listening quality preference damaged"));
                }
                let current = current.preferences;
                if blocked(tx, owner, &token, &current)? {
                    return Err(Error::State("listening quality trial blocks settings"));
                }
                if revision(epoch, &current, &choices)? != expected {
                    return Err(Error::State("listening quality settings changed"));
                }
                if !choices.iter().any(|choice| {
                    choice.quality == quality && choice.access == QualityAccess::Allowed
                }) {
                    return Err(Error::InvalidInput(
                        "playback quality preference is unavailable",
                    ));
                }
                let saved = if current.quality == quality {
                    current
                } else {
                    let next = QualityPreferences::fresh(quality);
                    QualityPreferences::replace_in(tx, owner, &current.revision, &next)?;
                    next
                };
                // The existing actor reloads this preference record on its next
                // tick/command. It does not reopen the currently held source.
                Ok(PlaybackQualityPreferencesView {
                    epoch: epoch.into(),
                    revision: Some(revision(epoch, &saved, &choices)?),
                    quality,
                    damaged: false,
                    choices,
                })
            })
        })
    }
}
