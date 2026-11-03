//! Read-only desktop projection. Native catalogue records may contain
//! authorized URLs and material data, so they must not cross IPC.
use super::{
    api::EffectsApi,
    catalog::{CatalogueKind, EffectId, Sound},
    rights::{RequiredMembership, tier},
    service::EffectsLibrary,
};
use crate::{Error, Result, account::SessionToken};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EffectDirectoryRow {
    pub id: EffectId,
    pub name: String,
    pub subtitle: String,
    /// Catalogue presentation only; never a fresh access decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub membership: Option<RequiredMembership>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trial_remaining_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_artwork: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dark_artwork: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectDirectoryAccess {
    Allowed,
    Trial,
    Unavailable,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EffectDirectoryDetail {
    #[serde(flatten)]
    pub effect: EffectDirectoryRow,
    pub access: EffectDirectoryAccess,
}

impl EffectDirectoryRow {
    fn from_sound(sound: Sound, now_ms: Option<u64>) -> Self {
        fn text(value: &str) -> String {
            value
                .trim()
                .chars()
                .filter(|c| !c.is_control())
                .take(512)
                .collect()
        }
        let auth = sound.catalogue_authorization();
        let membership = tier(auth.as_ref());
        let trial = &sound.native_record()["vipAuthRespItem"]["authResult"]["trialDto"];
        let trial_remaining_ms = match (membership, auth, now_ms) {
            (Some(_), Some(auth), Some(now)) if auth.catalogue_trial_active => trial["expireTime"]
                .as_u64()
                .filter(|time| *time <= 8_640_000_000_000_000)
                .and_then(|time| time.checked_sub(now))
                .filter(|left| *left > 0),
            _ => None,
        };
        Self {
            id: sound.id,
            name: text(&sound.name),
            subtitle: text(&sound.subtitle),
            membership,
            trial_remaining_ms,
            light_artwork: crate::protocol::artwork::original(&sound.native_record()["lightImg"]),
            dark_artwork: crate::protocol::artwork::original(&sound.native_record()["darkImg"]),
        }
    }
}

impl EffectsLibrary {
    async fn directory_time_for(&self, token: &SessionToken) -> Result<Option<u64>> {
        self.session
            .run_for(token, |api, _| async move {
                Ok(api.observed_server_time_ms().or_else(|| {
                    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
                    u64::try_from(elapsed.as_millis()).ok()
                }))
            })
            .await
    }

    pub async fn directory_for(
        &self,
        token: &SessionToken,
        kind: CatalogueKind,
    ) -> Result<Vec<EffectDirectoryRow>> {
        let rows = self.catalogue_for(token, kind, true).await?;
        let now = self.directory_time_for(token).await?;
        let rows = rows
            .into_iter()
            .map(|sound| EffectDirectoryRow::from_sound(sound, now))
            .collect();
        self.session.with_token(token, |_| Ok(rows))
    }

    pub async fn directory_detail_for(
        &self,
        token: &SessionToken,
        id: EffectId,
    ) -> Result<Option<EffectDirectoryDetail>> {
        let sound = self
            .session
            .run_for(token, |api, _| async move {
                EffectsApi::new(api).pc_detail(id).await
            })
            .await?;
        let Some(sound) = sound else {
            return self.session.with_token(token, |_| Ok(None));
        };
        // Query only for the explicitly opened PC detail. This is an observation
        // of current rights, not consent to register an interest or use a trial.
        let access = match self
            .session
            .run_for(token, |api, _| async move {
                EffectsApi::new(api).directory_authorization(id).await
            })
            .await
        {
            Ok(Some(auth)) if auth.can_use && auth.is_trial() => EffectDirectoryAccess::Trial,
            Ok(Some(auth)) if auth.can_use => EffectDirectoryAccess::Allowed,
            Ok(_) => EffectDirectoryAccess::Unavailable,
            Err(error)
                if !error.is_service_code(301)
                    && matches!(
                        error.root(),
                        Error::Transport(_)
                            | Error::RequestNotSent(_)
                            | Error::HttpStatus { .. }
                            | Error::Service { .. }
                            | Error::Response(_)
                            | Error::Protocol(_)
                    ) =>
            {
                EffectDirectoryAccess::Unknown
            }
            Err(error) => return Err(error),
        };
        let effect = EffectDirectoryRow::from_sound(sound, self.directory_time_for(token).await?);
        self.session.with_token(token, |_| {
            Ok(Some(EffectDirectoryDetail { effect, access }))
        })
    }
}
