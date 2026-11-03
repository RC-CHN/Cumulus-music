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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EffectDirectoryRow {
    pub id: EffectId,
    pub name: String,
    pub subtitle: String,
    /// Catalogue presentation only; never a fresh access decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub membership: Option<RequiredMembership>,
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

impl From<Sound> for EffectDirectoryRow {
    fn from(sound: Sound) -> Self {
        fn text(value: &str) -> String {
            value
                .trim()
                .chars()
                .filter(|c| !c.is_control())
                .take(512)
                .collect()
        }
        Self {
            id: sound.id,
            name: text(&sound.name),
            subtitle: text(&sound.subtitle),
            membership: tier(sound.catalogue_authorization().as_ref()),
            light_artwork: crate::protocol::artwork::original(&sound.native_record()["lightImg"]),
            dark_artwork: crate::protocol::artwork::original(&sound.native_record()["darkImg"]),
        }
    }
}

impl EffectsLibrary {
    pub async fn directory_for(
        &self,
        token: &SessionToken,
        kind: CatalogueKind,
    ) -> Result<Vec<EffectDirectoryRow>> {
        let rows = self
            .catalogue_for(token, kind, true)
            .await?
            .into_iter()
            .map(EffectDirectoryRow::from)
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
        let effect = sound.into();
        self.session.with_token(token, |_| {
            Ok(Some(EffectDirectoryDetail { effect, access }))
        })
    }
}
