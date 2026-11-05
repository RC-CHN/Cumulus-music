//! M/3097 download settings. A saved preference does not grant media rights.
use super::{MusicClient, PREFERENCES};
use crate::{
    Error, Result,
    account::SessionToken,
    download::service::DownloadOptions,
    membership::{MembershipManager, MembershipState, Quality},
    storage::StateTransaction,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

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
