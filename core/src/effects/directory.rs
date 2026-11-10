//! Read-only desktop projection. Native catalogue records may contain
//! authorized URLs and material data, so they must not cross IPC.
use super::{
    api::EffectsApi,
    catalog::{CatalogueKind, EffectId, Sound},
    rights::{RequiredMembership, tier},
    service::EffectsLibrary,
};
use crate::{
    Error, Result,
    account::SessionToken,
    protocol::api::{Api, EffectCatalogueRead},
};
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
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub newly_added: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectCatalogueLayout {
    Legacy,
    Modern,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectDirectorySection {
    Selection,
    More,
    LegacyRecommended,
    LegacyAll,
}

#[derive(Serialize)]
pub struct EffectDirectoryGroup {
    pub key: EffectDirectorySection,
    pub items: Vec<EffectDirectoryRow>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectDirectoryOverview {
    pub cohort_revision: String,
    pub layout: EffectCatalogueLayout,
    pub groups: Vec<EffectDirectoryGroup>,
    pub partial: bool,
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
            newly_added: false,
        }
    }

    fn from_legacy_sound(sound: Sound) -> Self {
        // M/3091's old D card consumes title, optional subtitle, type===3
        // and newAdd===1. It does not use M/21's imgUrl or material link.
        let record = sound.native_record();
        let vip = record["type"].as_u64() == Some(3);
        let newly_added = record["newAdd"].as_u64() == Some(1);
        let mut row = Self::from_sound(sound, None);
        row.membership = vip.then_some(RequiredMembership::Vip);
        row.light_artwork = None;
        row.dark_artwork = None;
        row.newly_added = newly_added;
        row
    }
}

fn invalid_scope(error: &Error) -> bool {
    error.is_service_code(301)
        || matches!(
            error.root(),
            Error::StaleResponse | Error::CredentialChanged | Error::Cancelled
        )
}

impl EffectsLibrary {
    pub(crate) async fn directory_overview_for(
        &self,
        token: &SessionToken,
        cohort_revision: String,
        layout: EffectCatalogueLayout,
        credential: Option<&str>,
        current: impl Fn() -> Result<()> + Send + Sync,
    ) -> Result<EffectDirectoryOverview> {
        current()?;
        let current = &current;
        let (groups, partial) = match layout {
            EffectCatalogueLayout::Legacy => {
                // M/3091/H always awaits promote before asking for all.
                let (mut recommended, mut all) = self
                    .session
                    .run_for(token, |api, _| async {
                        let api = EffectsApi::new(api);
                        let recommended = legacy_rows(
                            api.directory_catalogue(
                                EffectCatalogueRead::LegacyRecommended,
                                credential,
                                current,
                            )
                            .await,
                        )?;
                        current()?;
                        let all = legacy_rows(
                            api.directory_catalogue(
                                EffectCatalogueRead::LegacyAll,
                                credential,
                                current,
                            )
                            .await,
                        )?;
                        Ok((recommended, all))
                    })
                    .await?;
                if all.is_empty() {
                    recommended.clear();
                } else {
                    recommended.truncate(6);
                    all.retain(|sound| !recommended.iter().any(|item| item.id == sound.id));
                }
                (
                    vec![
                        EffectDirectoryGroup {
                            key: EffectDirectorySection::LegacyRecommended,
                            items: recommended
                                .into_iter()
                                .map(EffectDirectoryRow::from_legacy_sound)
                                .collect(),
                        },
                        EffectDirectoryGroup {
                            key: EffectDirectorySection::LegacyAll,
                            items: all
                                .into_iter()
                                .map(EffectDirectoryRow::from_legacy_sound)
                                .collect(),
                        },
                    ],
                    false,
                )
            }
            EffectCatalogueLayout::Modern => {
                self.session
                    .run_for(token, |api, _| async move {
                        let effects = EffectsApi::new(api.clone());
                        let (high, recommended, more) = tokio::join!(
                            effects.directory_catalogue(
                                EffectCatalogueRead::HighlyRecommended,
                                credential,
                                current
                            ),
                            effects.directory_catalogue(
                                EffectCatalogueRead::Recommended,
                                credential,
                                current
                            ),
                            effects.directory_catalogue(
                                EffectCatalogueRead::More,
                                credential,
                                current
                            ),
                        );
                        modern_overview([high, recommended, more], directory_now(api.as_ref()))
                    })
                    .await?
            }
        };
        current()?;
        Ok(EffectDirectoryOverview {
            cohort_revision,
            layout,
            groups,
            partial,
        })
    }

    async fn directory_time_for(&self, token: &SessionToken) -> Result<Option<u64>> {
        self.session
            .run_for(
                token,
                |api, _| async move { Ok(directory_now(api.as_ref())) },
            )
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

fn directory_now(api: &dyn Api) -> Option<u64> {
    api.observed_server_time_ms().or_else(|| {
        let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
        u64::try_from(elapsed.as_millis()).ok()
    })
}

fn modern_overview(
    parts: [Result<Vec<Sound>>; 3],
    now: Option<u64>,
) -> Result<(Vec<EffectDirectoryGroup>, bool)> {
    let failures = parts.iter().filter(|part| part.is_err()).count();
    if failures == parts.len()
        || parts
            .iter()
            .any(|part| part.as_ref().is_err_and(invalid_scope))
    {
        let mut errors = parts
            .into_iter()
            .filter_map(Result::err)
            .collect::<Vec<_>>();
        let index = errors.iter().position(invalid_scope).unwrap_or(0);
        return Err(errors.swap_remove(index));
    }
    let [high, recommended, more] = parts.map(Result::unwrap_or_default);
    let mut groups = Vec::new();
    let row = |sound| EffectDirectoryRow::from_sound(sound, now);
    if !high.is_empty() && recommended.len() > 1 {
        groups.push(EffectDirectoryGroup {
            key: EffectDirectorySection::Selection,
            items: high
                .into_iter()
                .chain(recommended)
                .take(7)
                .map(row)
                .collect(),
        });
    }
    if !more.is_empty() {
        groups.push(EffectDirectoryGroup {
            key: EffectDirectorySection::More,
            items: more.into_iter().map(row).collect(),
        });
    }
    Ok((groups, failures > 0))
}

fn legacy_rows(result: Result<Vec<Sound>>) -> Result<Vec<Sound>> {
    match result {
        Ok(rows) => Ok(rows),
        Err(error) if invalid_scope(&error) => Err(error),
        Err(_) => Ok(Vec::new()),
    }
}
