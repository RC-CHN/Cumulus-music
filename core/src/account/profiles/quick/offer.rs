//! PC quick-login history participation is separate from the startup and local
//! restore record. Store only authorization and whitelisted profile metadata.
use super::{OWNER, ProfileRegistry, public_avatar, removal, saved, text};
use crate::{
    Error, Result,
    account::{AccountSession, SessionStatus, SessionToken, login_policy::AB_PATH},
    protocol::{
        api::{Api, identifier},
        http::{ApiOrigin, MusicHttpClient, WireRoute},
    },
    storage::{OperationLock, StateTransaction},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{any::Any, sync::Arc, time::Duration};

const EXPERIMENT: &str = "PH-PC-quickLoginSwitchAccountNew";
const OFFERS: &str = "quick-login-offer-v1";
const ELIGIBILITY: &str = "/api/login/getquickloginlist";

struct OfferContext {
    uid: String,
    selected: super::super::Selection,
    api: Arc<dyn Api>,
    native: Arc<MusicHttpClient>,
    credential_revision: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OfferProfile {
    pub(super) nickname: String,
    pub(super) avatar_url: String,
    pub(super) avatar_img_id: Value,
    pub(super) user_type: u32,
    pub(super) cellphone: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::account::profiles) struct Record {
    pub(super) revision: String,
    pub(super) profile: Option<OfferProfile>,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickLoginOfferView {
    pub enabled: bool,
    pub revision: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickLoginAccessView {
    pub assigned: bool,
    pub offer: Option<QuickLoginOfferView>,
}

pub(super) fn read(tx: &StateTransaction<'_>, uid: &str) -> Result<Option<Record>> {
    tx.get(OWNER, OFFERS, uid)?
        .map(|raw| {
            let record: Record = serde_json::from_value(raw)?;
            if !super::super::id(&record.revision) {
                return Err(Error::State("invalid quick login offer revision"));
            }
            Ok(record)
        })
        .transpose()
}

pub(in crate::account::profiles) fn confirm(
    tx: &StateTransaction<'_>,
    uid: &str,
    expected: &Record,
) -> Result<()> {
    if read(tx, uid)?.as_ref() != Some(expected) {
        return Err(Error::StaleResponse);
    }
    Ok(())
}

pub(super) fn invalidate(
    tx: &mut StateTransaction<'_>,
    uid: &str,
    expected: &Record,
) -> Result<()> {
    confirm(tx, uid, expected)?;
    // A tombstone invalidates any open Settings switch/prepared exchange.
    // Nothing in saved profiles, credential stores or the active selection is removed.
    let next = Record {
        revision: uuid::Uuid::new_v4().simple().to_string(),
        profile: None,
    };
    tx.put(OWNER, OFFERS, uid, &serde_json::to_value(next)?)
}

pub(in crate::account::profiles) fn forget(tx: &mut StateTransaction<'_>, uid: &str) -> Result<()> {
    tx.delete(OWNER, OFFERS, uid)
}

fn assigned(value: &Value) -> bool {
    let Some(rows) = value["data"]
        .as_array()
        .filter(|rows| rows.len() <= 4096 && rows.iter().all(Value::is_object))
    else {
        return false;
    };
    let mut matches = rows.iter().filter(|row| row["expName"] == EXPERIMENT);
    matches
        .next()
        .is_some_and(|row| row["expGroupName"] == "t1")
        && matches.next().is_none()
}

fn view(record: Option<Record>) -> QuickLoginOfferView {
    QuickLoginOfferView {
        enabled: record
            .as_ref()
            .is_some_and(|record| record.profile.is_some()),
        revision: record.map(|record| record.revision),
    }
}

impl ProfileRegistry {
    pub(super) fn quick_offer_lock(&self) -> Result<OperationLock> {
        OperationLock::acquire(&self.root, OFFERS, "history")
    }

    pub(crate) async fn quick_login_assignment(
        &self,
        session: &AccountSession,
        token: &SessionToken,
    ) -> Result<bool> {
        let snapshot = session.snapshot()?;
        if snapshot.token != *token {
            return Err(Error::StaleResponse);
        }
        if self.origin() != ApiOrigin::Pc || snapshot.status == SessionStatus::Expired {
            return Ok(false);
        }
        let api = session.native_api(token)?;
        // Missing, ambiguous and failed reads are the official closed gate.
        let response = tokio::time::timeout(
            Duration::from_secs(4),
            api.call(AB_PATH, json!({"expNames":EXPERIMENT}), false, &[200]),
        )
        .await;
        session.require_api(token, &api)?;
        Ok(matches!(response, Ok(Ok(value)) if assigned(&value)))
    }

    pub(crate) async fn quick_login_access(
        &self,
        session: &AccountSession,
        token: &SessionToken,
    ) -> Result<QuickLoginAccessView> {
        let assigned = self.quick_login_assignment(session, token).await?;
        let offer = if assigned && let Some(context) = self.offer_context(session, token)? {
            let current = self.store.transaction(|tx| {
                Self::check_offer_selection(tx, &context.uid, &context.selected)?;
                read(tx, &context.uid)
            })?;
            session.require_api(token, &context.api)?;
            Some(view(current))
        } else {
            None
        };
        Ok(QuickLoginAccessView { assigned, offer })
    }

    fn offer_context(
        &self,
        session: &AccountSession,
        token: &SessionToken,
    ) -> Result<Option<OfferContext>> {
        let Some(uid) = token.uid.clone() else {
            return Ok(None);
        };
        let Some(selected) = self.selection()? else {
            return Ok(None);
        };
        let api = session.native_api(token)?;
        let native: Arc<dyn Any + Send + Sync> = api.clone();
        let Some(native) = native.downcast::<MusicHttpClient>().ok() else {
            return Ok(None);
        };
        if native.credentials().store().directory() != self.profile_path(&selected.session_id)? {
            return Ok(None);
        }
        let target = self.origin().request_url(WireRoute::Eapi, ELIGIBILITY)?;
        let credentials = native.credentials().request_credentials(&target)?;
        if credentials.unique_music_u().is_none() {
            return Ok(None);
        }
        Ok(Some(OfferContext {
            uid,
            selected,
            api,
            native,
            credential_revision: credentials.revision,
        }))
    }

    fn check_offer_selection(
        tx: &StateTransaction<'_>,
        uid: &str,
        selected: &super::super::Selection,
    ) -> Result<()> {
        if tx.get(OWNER, super::super::KIND, "active")? != Some(serde_json::to_value(selected)?) {
            return Err(Error::StaleResponse);
        }
        removal::available(tx, &selected.session_id)?;
        let saved: super::super::SavedProfile = serde_json::from_value(
            tx.get(OWNER, saved::KIND, uid)?
                .ok_or(Error::StaleResponse)?,
        )?;
        if saved.uid != uid || saved.session_id != selected.session_id {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    pub(crate) async fn set_quick_login_offer(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        enabled: bool,
        expected_revision: Option<&str>,
    ) -> Result<QuickLoginOfferView> {
        if !self.quick_login_assignment(session, token).await? {
            return Err(Error::State("quick login offer is unavailable"));
        }
        let context = self.offer_context(session, token)?.ok_or(Error::State(
            "quick login offer requires a native current profile",
        ))?;
        let profile = if enabled {
            let mut changes = session.subscribe()?;
            session.require_api(token, &context.api)?;
            let fetch = tokio::time::timeout(
                Duration::from_secs(12),
                enrollment_profile(&*context.api, &context.uid),
            );
            let metadata = tokio::select! {
                biased;
                _ = changes.changed() => return Err(Error::StaleResponse),
                response = fetch => response.map_err(|_| Error::Transport("quick login offer metadata timeout".into()))??,
            };
            Some(metadata)
        } else {
            None
        };
        let _lock = self.quick_offer_lock()?;
        session.with_verified_profile(token, &context.api, |owner, _| {
            if owner != context.uid {
                return Err(Error::StaleResponse);
            }
            let target = self.origin().request_url(WireRoute::Eapi, ELIGIBILITY)?;
            let latest = context.native.credentials().request_credentials(&target)?;
            if latest.revision != context.credential_revision || latest.unique_music_u().is_none() {
                return Err(Error::CredentialChanged);
            }
            self.store.transaction(|tx| {
                Self::check_offer_selection(tx, owner, &context.selected)?;
                let prior = read(tx, owner)?;
                if prior.as_ref().map(|old| old.revision.as_str()) != expected_revision {
                    return Err(Error::StaleResponse);
                }
                if prior.as_ref().and_then(|old| old.profile.as_ref()) == profile.as_ref() {
                    return Ok(view(prior));
                }
                let next = Record {
                    revision: uuid::Uuid::new_v4().simple().to_string(),
                    profile,
                };
                tx.put(OWNER, OFFERS, owner, &serde_json::to_value(&next)?)?;
                Ok(view(Some(next)))
            })
        })
    }
}

async fn enrollment_profile(api: &dyn Api, uid: &str) -> Result<OfferProfile> {
    // Settings Account.jsx reads bindings first; M198 then fetches detail, VIP
    // and identity concurrently. Only the bounded history profile is saved.
    let binding = api
        .call(
            &format!("/api/w/v1/user/bindings/{uid}"),
            json!({"userId":uid}),
            false,
            &[200],
        )
        .await?;
    let detail_path = format!("/api/w/v1/user/detail/{uid}");
    let (detail, vip, identities) = tokio::try_join!(
        api.call(
            &detail_path,
            json!({"all":true,"userId":uid}),
            false,
            &[200]
        ),
        api.call(
            "/api/music-vip-membership/client/vip/info",
            json!({"userId":uid}),
            false,
            &[200]
        ),
        api.call(
            "/api/authentication/identity/user/all",
            json!({"userId":uid}),
            false,
            &[200]
        ),
    )?;
    let profile = &detail["profile"];
    if identifier(&profile["userId"]).as_deref().ok() != Some(uid)
        || !vip["data"].is_object()
        || (vip["data"].get("userId").is_some()
            && identifier(&vip["data"]["userId"]).as_deref().ok() != Some(uid))
        || !identities["data"].is_array()
    {
        return Err(Error::Response(
            "quick login offer metadata identity mismatch",
        ));
    }
    let mut phone = String::new();
    let bindings = binding["bindings"]
        .as_array()
        .filter(|rows| rows.len() <= 32)
        .ok_or(Error::Response("invalid quick login offer bindings"))?;
    for row in bindings.iter().filter(|row| row["type"] == 1) {
        if let Some(value) = row["tokenJsonStr"]
            .as_str()
            .filter(|raw| raw.len() <= 4096)
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        {
            let candidate = text(value["cellphone"].as_str(), 32);
            if candidate.bytes().all(|byte| byte.is_ascii_digit()) {
                phone = candidate.into();
            }
        }
    }
    let avatar_img_id = &profile["avatarImgId"];
    let avatar_img_id = if identifier(avatar_img_id).is_ok_and(|id| id.len() <= 32) {
        avatar_img_id.clone()
    } else {
        json!(0)
    };
    Ok(OfferProfile {
        nickname: text(profile["nickname"].as_str(), 512).into(),
        avatar_url: public_avatar(profile["avatarUrl"].as_str()),
        avatar_img_id,
        user_type: profile["userType"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0),
        cellphone: phone,
    })
}
