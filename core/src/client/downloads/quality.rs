//! M/3097 download settings. A saved preference does not grant media rights.
use super::{MusicClient, PREFERENCES};
use crate::{
    Error, Result,
    account::SessionToken,
    download::service::DownloadOptions,
    membership::{MembershipCommit, MembershipManager, MembershipState, Quality},
    storage::StateTransaction,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::time::Instant;

const MEMBERSHIP_READ_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Access {
    Allowed,
    RequiresVip,
    RequiresSvip,
    Unknown,
}

#[derive(Serialize)]
struct Choice {
    quality: Quality,
    access: Access,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadQualityPreferences {
    pub epoch: String,
    pub revision: String,
    pub options: DownloadOptions,
    choices: Vec<Choice>,
}

fn choices(
    membership: Option<&MembershipState>,
    music_package_hires: Option<bool>,
    vivid: bool,
) -> Vec<Choice> {
    use Quality::{ExHigh, HiRes, JyEffect, JyMaster, Lossless, Sky, Standard, Vivid};
    [
        Vivid, Sky, JyMaster, JyEffect, HiRes, Lossless, ExHigh, Standard,
    ]
    .into_iter()
    .filter(|quality| *quality != Vivid || vivid)
    .map(|quality| {
        let access = if matches!(quality, Standard | ExHigh) {
            Access::Allowed
        } else if let Some(member) = membership {
            if member.is_plus_vip {
                Access::Allowed
            } else if matches!(quality, Vivid | Sky | JyMaster) {
                Access::RequiresSvip
            } else if member.is_vinyl_vip {
                Access::Allowed
            } else if member.is_music_package && matches!(quality, Lossless | HiRes) {
                match music_package_hires {
                    Some(true) => Access::Allowed,
                    Some(false) => Access::RequiresVip,
                    None => Access::Unknown,
                }
            } else {
                Access::RequiresVip
            }
        } else {
            Access::Unknown
        };
        Choice { quality, access }
    })
    .collect()
}

fn options(tx: &StateTransaction<'_>, owner: &str) -> Result<DownloadOptions> {
    Ok(tx
        .get(owner, PREFERENCES, "options")?
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default())
}

fn revision(epoch: &str, options: &DownloadOptions, choices: &[Choice]) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(
        epoch, options, choices,
    ))?)))
}

fn download_membership_expired(commit: &MembershipCommit, now: Instant) -> bool {
    let server_now =
        commit.server_now + now.saturating_duration_since(commit.at).as_secs_f64() * 1000.0;
    // MembershipManager::expiries orders vinyl, plus, music package, album, voice book.
    let [vinyl, plus, music_package, _, _] = commit.expiries;
    [plus, vinyl, music_package]
        .into_iter()
        .any(|expiry| expiry > commit.server_now && expiry <= server_now)
}

impl MusicClient {
    fn download_quality_token(&self, epoch: &str) -> Result<SessionToken> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(token)
    }

    fn download_quality_choices(&self, token: &SessionToken) -> Result<Vec<Choice>> {
        let api = self.session.native_api(token)?;
        // Only the current credential's live observation supplies Vivid.
        // Opening settings does not dispatch a new experiment or trial.
        let vivid = if let Ok(Some(credential)) = api.verified_pc_experiment_revision() {
            self.pc_experiments
                .observed_groups(token, &api, &credential)?
                .is_some_and(|groups| {
                    groups.group("PH-PC-hyyinzhi") == Some("t1")
                        && groups.group("PH-PC-zhenyingn") == Some("t1")
                })
        } else {
            false
        };
        let config = self
            .auth
            .context
            .as_ref()
            .and_then(|context| context.pc_snapshot().ok().flatten());
        let music_package_hires = config
            .as_ref()
            .and_then(|snapshot| super::super::player::music_package_hires(snapshot.data.as_ref()));
        let membership = MembershipManager::new(self.session.clone())?
            .snapshot()
            .ok();
        self.session.with_token(token, |_| Ok(()))?;
        Ok(choices(
            membership.as_ref().map(|snapshot| &snapshot.state),
            music_package_hires,
            vivid,
        ))
    }

    pub fn download_quality_preferences(&self, epoch: &str) -> Result<DownloadQualityPreferences> {
        let token = self.download_quality_token(epoch)?;
        let choices = self.download_quality_choices(&token)?;
        let store = self.session.persistent_store()?;
        self.session.with_token(&token, |owner| {
            let options = store.read_snapshot(|tx| options(tx, owner))?;
            Ok(DownloadQualityPreferences {
                epoch: epoch.into(),
                revision: revision(epoch, &options, &choices)?,
                options,
                choices,
            })
        })
    }

    /// Explicit settings retry only. Reuse the startup/expiry worker's native
    /// reader and its coalescing gate; opening the form is still passive.
    pub async fn refresh_download_quality_preferences(
        &self,
        epoch: &str,
    ) -> Result<DownloadQualityPreferences> {
        let token = self.download_quality_token(epoch)?;
        let mut accounts = self.session.subscribe()?;
        let commits = self.session.subscribe_membership_commits()?;
        let due = commits
            .borrow()
            .as_ref()
            .filter(|commit| commit.token == token)
            .is_none_or(|commit| download_membership_expired(commit, Instant::now()))
            || MembershipManager::new(self.session.clone())?
                .snapshot()
                .is_err();
        if due {
            let manager = self.membership_manager().await?;
            let refresh =
                tokio::time::timeout(MEMBERSHIP_READ_TIMEOUT, manager.refresh_for(token.clone()));
            tokio::select! {
                biased;
                _ = accounts.wait_for(|current| current != &token) => return Err(Error::StaleResponse),
                result = refresh => result.map_err(|_| Error::Transport("download membership read timed out".into()))??,
            };
        }
        self.download_quality_preferences(epoch)
    }

    pub(super) fn save_download_quality_preferences(
        &self,
        epoch: &str,
        expected: &str,
        desired: &DownloadOptions,
    ) -> Result<()> {
        let token = self.download_quality_token(epoch)?;
        let choices = self.download_quality_choices(&token)?;
        let store = self.session.persistent_store()?;
        self.session.with_token(&token, |owner| {
            store.transaction(|tx| {
                let current = options(tx, owner)?;
                if revision(epoch, &current, &choices)? != expected {
                    return Err(Error::QueueChanged);
                }
                // Unavailable legacy selections survive changes to other
                // download settings; they cannot be selected anew.
                if desired.quality != current.quality
                    && !choices.iter().any(|choice| {
                        choice.quality == desired.quality && choice.access == Access::Allowed
                    })
                {
                    return Err(Error::InvalidInput(
                        "download quality preference is unavailable",
                    ));
                }
                tx.put(
                    owner,
                    PREFERENCES,
                    "options",
                    &serde_json::to_value(desired)?,
                )
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::membership::membership_state;
    use serde_json::json;

    #[test]
    fn download_membership_rechecks_crossed_not_already_known_expiry() {
        let at = Instant::now();
        for index in 0..5 {
            let mut expiries = [0.0; 5];
            expiries[index] = 1_100.0;
            let commit = MembershipCommit {
                token: SessionToken {
                    generation: 1,
                    uid: Some("1".into()),
                },
                expiries,
                server_now: 1_000.0,
                at,
            };
            assert!(!download_membership_expired(
                &commit,
                at + Duration::from_millis(99)
            ));
            assert_eq!(
                download_membership_expired(&commit, at + Duration::from_millis(100)),
                index < 3,
                "membership slot {index}"
            );
            let checked_expired = MembershipCommit {
                server_now: 1_110.0,
                ..commit
            };
            assert!(!download_membership_expired(
                &checked_expired,
                at + Duration::from_secs(10)
            ));
        }
    }

    #[test]
    fn music_package_only_applies_to_the_two_recovered_download_qualities() {
        let member =
            membership_state(&json!({"musicPackage":{"expireTime":3000}}), 2000.0).unwrap();
        for (configured, expected) in [
            (Some(true), Access::Allowed),
            (Some(false), Access::RequiresVip),
            (None, Access::Unknown),
        ] {
            let choices = choices(Some(&member), configured, true);
            let access = |quality| {
                choices
                    .iter()
                    .find(|row| row.quality == quality)
                    .unwrap()
                    .access
            };
            assert_eq!(access(Quality::Lossless), expected);
            assert_eq!(access(Quality::HiRes), expected);
            assert_eq!(access(Quality::JyEffect), Access::RequiresVip);
            assert_eq!(access(Quality::Vivid), Access::RequiresSvip);
        }
    }
}
