//! Explicit JSK eligibility check. It never issues a login or changes profiles.
mod exchange;
pub(super) mod offer;
use super::{OWNER, ProfileRegistry, SavedProfile, Selection, removal, saved};
use crate::{
    Error, Result,
    account::{
        AccountSession, SessionToken,
        credentials::{Credentials, QuickLoginCredential},
    },
    protocol::{
        api::identifier,
        http::{ApiOrigin, MusicHttpClient, WireRoute},
    },
};
pub use exchange::{
    QuickLoginTransport, ServerQuickLogin, ServerQuickLoginPhase, ServerQuickLoginView,
};
pub use offer::{QuickLoginAccessView, QuickLoginOfferView};
use serde::Serialize;
use serde_json::{Value, json};
use std::{any::Any, cmp::Reverse, collections::BTreeSet, sync::Arc};
use url::Url;

const PATH: &str = "/api/login/getquickloginlist";
const MAX_HISTORY: usize = 3;
const MAX_LOCAL_ROWS: usize = 32;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickLoginEligibilityView {
    /// Only accounts actually submitted for this check. Other saved accounts
    /// have unknown eligibility; an empty result is not a remote rejection.
    pub accounts: Vec<QuickLoginAccountView>,
    pub checked_count: usize,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickLoginAccountView {
    pub id: String,
    pub current: bool,
    pub eligible: bool,
}

struct Entry {
    saved: SavedProfile,
    credential: QuickLoginCredential,
    offer: offer::Record,
    current: bool,
}

struct History {
    selection: Option<Selection>,
    entries: Vec<Entry>,
}

pub(super) struct QuickSavedTarget {
    pub(super) saved: SavedProfile,
    credential_revision: String,
    pub(super) offer: offer::Record,
}

impl ProfileRegistry {
    pub async fn quick_login_eligibility_for(
        self: &Arc<Self>,
        session: &AccountSession,
        expected: &SessionToken,
    ) -> Result<QuickLoginEligibilityView> {
        self.checked_quick_history(session, expected)
            .await
            .map(|(_, view)| view)
    }

    async fn checked_quick_history(
        self: &Arc<Self>,
        session: &AccountSession,
        expected: &SessionToken,
    ) -> Result<(History, QuickLoginEligibilityView)> {
        if self.origin() != ApiOrigin::Pc {
            return Err(Error::InvalidInput(
                "JSK quick login requires the PC origin",
            ));
        }
        let _offer_lock = self.quick_offer_lock()?;
        let api = session.native_api(expected)?;
        let target = ApiOrigin::Pc.request_url(WireRoute::Eapi, PATH)?;
        let (history, payload) = {
            let (registry, target, uid) = (self.clone(), target.clone(), expected.uid.clone());
            tokio::task::spawn_blocking(move || {
                let history = registry.quick_history(uid.as_deref(), &target)?;
                let payload = registry.quick_payload(&history)?;
                registry.check_quick_history(&history, &target)?;
                Ok::<_, Error>((history, payload))
            })
            .await
            .map_err(|_| Error::State("quick history worker stopped"))??
        };
        session.require_api(expected, &api)?;
        if history.entries.is_empty() {
            return Ok((
                history,
                QuickLoginEligibilityView {
                    accounts: Vec::new(),
                    checked_count: 0,
                },
            ));
        }
        let native: Arc<dyn Any + Send + Sync> = api.clone();
        let native = native
            .downcast::<MusicHttpClient>()
            .map_err(|_| Error::State("native JSK quick login transport unavailable"))?;
        let revision = native.credentials().request_credentials(&target)?.revision;
        let mut changes = session.subscribe()?;
        session.require_api(expected, &api)?;
        // Guest and authenticated pickers may both ask explicitly. Do not use
        // run_for: this list cannot expire or adopt the current account.
        let reply = tokio::select! {
            biased;
            _ = changes.changed() => return Err(Error::StaleResponse),
            result = native.request_jsk_quick_eligibility(payload, &revision) => result,
        };
        session.require_api(expected, &api)?;
        if native.credentials().request_credentials(&target)?.revision != revision {
            return Err(Error::CredentialChanged);
        }
        let view = project_reply(
            reply.map_err(|error| {
                // Never return an upstream body which could echo history secrets.
                match error.root() {
                    Error::CredentialChanged => Error::CredentialChanged,
                    Error::Response(_) => {
                        Error::Response("invalid quick login eligibility response")
                    }
                    _ => Error::Transport("quick login eligibility request failed".into()),
                }
            })?,
            &history.entries,
        )?;
        let mut history = {
            let (registry, target) = (self.clone(), target.clone());
            tokio::task::spawn_blocking(move || {
                registry.check_quick_history(&history, &target)?;
                Ok::<_, Error>(history)
            })
            .await
            .map_err(|_| Error::State("quick history worker stopped"))??
        };
        session.require_api(expected, &api)?;
        // Keep the generation check and local invalidation together. The
        // bounded transaction does not start HTTP or clear credentials.
        session.with_local_owner(expected, |_| {
            if native.credentials().request_credentials(&target)?.revision != revision {
                return Err(Error::CredentialChanged);
            }
            self.apply_quick_invalidations(&history, &view)
        })?;
        history.entries.retain(|entry| {
            view.accounts
                .iter()
                .any(|row| row.id == entry.saved.session_id && row.eligible)
        });
        Ok((history, view))
    }

    fn apply_quick_invalidations(
        &self,
        history: &History,
        view: &QuickLoginEligibilityView,
    ) -> Result<()> {
        if !view.accounts.iter().any(|row| !row.eligible) {
            return Ok(());
        }
        // M1368 calls the native historical-account deletion before publishing
        // its usable list. Our history is an offer, never the saved login store.
        self.store.transaction(|tx| {
            let selection = tx.get(OWNER, super::KIND, "active")?;
            if selection
                != history
                    .selection
                    .as_ref()
                    .map(serde_json::to_value)
                    .transpose()?
            {
                return Err(Error::StaleResponse);
            }
            for (entry, row) in history.entries.iter().zip(&view.accounts) {
                if entry.saved.session_id != row.id {
                    return Err(Error::StaleResponse);
                }
                if !row.eligible {
                    removal::available(tx, &entry.saved.session_id)?;
                    if tx.get(OWNER, saved::KIND, &entry.saved.uid)?
                        != Some(serde_json::to_value(&entry.saved)?)
                    {
                        return Err(Error::StaleResponse);
                    }
                    offer::invalidate(tx, &entry.saved.uid, &entry.offer)?;
                }
            }
            Ok(())
        })
    }

    fn quick_history(&self, current: Option<&str>, target: &Url) -> Result<History> {
        let selection = self.selection()?;
        let rows = self.store.transaction(|tx| {
            let mut rows = tx.values_page(OWNER, saved::KIND, None, MAX_LOCAL_ROWS)?;
            if let Some(uid) = current
                && !rows.contains_key(uid)
                && let Some(row) = tx.get(OWNER, saved::KIND, uid)?
            {
                rows.insert(uid.into(), row);
            }
            rows.into_iter()
                .filter_map(|(uid, value)| {
                    let parsed = (|| {
                        let saved: SavedProfile = serde_json::from_value(value)
                            .map_err(|_| Error::State("invalid saved quick login history"))?;
                        if saved.uid != uid
                            || !super::id(&saved.session_id)
                            || identifier(&json!(uid))? != uid
                        {
                            return Err(Error::State("invalid saved quick login identity"));
                        }
                        removal::available(tx, &saved.session_id)?;
                        Ok(offer::read(tx, &uid)?
                            .filter(|value| value.profile.is_some())
                            .map(|offer| (saved, offer)))
                    })();
                    match parsed {
                        Err(Error::State("saved account removal has started")) | Ok(None) => None,
                        Ok(Some(entry)) => Some(Ok(entry)),
                        Err(error) => Some(Err(error)),
                    }
                })
                .collect::<Result<Vec<_>>>()
        })?;
        let mut entries = Vec::new();
        let mut session_ids = BTreeSet::new();
        for (saved, offer) in rows {
            if !session_ids.insert(saved.session_id.clone()) {
                return Err(Error::State("duplicate saved quick login session"));
            }
            let database = self.profile_path(&saved.session_id)?.join("core.sqlite3");
            if let Some(credential) =
                Credentials::quick_login_credential(&database, self.origin(), target)?
            {
                entries.push(Entry {
                    current: current == Some(saved.uid.as_str()),
                    saved,
                    credential,
                    offer,
                });
            }
        }
        if current.is_some() && !entries.iter().any(|entry| entry.current) {
            entries.clear();
        }
        // M1368 limits the SDK history to three. The PC bridge synthesizes
        // lastUsedDate for each record (M290), not durable usage timestamps.
        // Here prefer the active saved account, then actual login receipts;
        // older profiles without receipts have a deterministic UID order.
        entries.sort_by(|a, b| {
            (
                Reverse(a.current),
                Reverse(a.credential.last_login_at),
                &a.saved.uid,
            )
                .cmp(&(
                    Reverse(b.current),
                    Reverse(b.credential.last_login_at),
                    &b.saved.uid,
                ))
        });
        entries.truncate(MAX_HISTORY);
        Ok(History { selection, entries })
    }

    fn quick_payload(&self, history: &History) -> Result<Value> {
        let rows = history
            .entries
            .iter()
            .map(|entry| {
                let saved = &entry.saved;
                let profile = entry.offer.profile.as_ref().ok_or(Error::StaleResponse)?;
                Ok(json!({
                    "userId": saved.uid,
                    "userProfile": {
                        "userId": saved.uid,
                        "nickname": profile.nickname,
                        "avatarUrl": profile.avatar_url,
                        "avatarImgId": profile.avatar_img_id,
                        "userType": profile.user_type,
                        "cellphone": profile.cellphone,
                    },
                    "musicU": entry.credential.music_u,
                }))
            })
            .collect::<Result<Vec<_>>>()?;
        // M1368:4350–4375. JSK is deliberately separate from legacy's richer
        // currentEnv and usableUserIds response.
        Ok(json!({"currentEnv":"{}","userInfoList":serde_json::to_string(&rows)?}))
    }

    fn check_quick_history(&self, history: &History, target: &Url) -> Result<()> {
        if self.selection()? != history.selection {
            return Err(Error::StaleResponse);
        }
        for entry in &history.entries {
            let stored = self.store.transaction(|tx| {
                removal::available(tx, &entry.saved.session_id)?;
                offer::confirm(tx, &entry.saved.uid, &entry.offer)?;
                tx.get(OWNER, saved::KIND, &entry.saved.uid)
            })?;
            if stored.as_ref() != Some(&serde_json::to_value(&entry.saved)?) {
                return Err(Error::StaleResponse);
            }
            let database = self
                .profile_path(&entry.saved.session_id)?
                .join("core.sqlite3");
            if !Credentials::quick_login_credential(&database, self.origin(), target)?.is_some_and(
                |value| {
                    value.revision == entry.credential.revision
                        && value.music_u == entry.credential.music_u
                },
            ) {
                return Err(Error::CredentialChanged);
            }
        }
        Ok(())
    }

    pub(super) fn check_quick_target(&self, binding: &QuickSavedTarget) -> Result<()> {
        let target = self.origin().request_url(WireRoute::Eapi, PATH)?;
        let database = self
            .profile_path(&binding.saved.session_id)?
            .join("core.sqlite3");
        if Credentials::quick_login_credential(&database, self.origin(), &target)?
            .is_none_or(|credential| credential.revision != binding.credential_revision)
        {
            return Err(Error::CredentialChanged);
        }
        Ok(())
    }
}

fn text(value: Option<&str>, limit: usize) -> &str {
    value
        .filter(|s| s.len() <= limit && !s.chars().any(char::is_control))
        .unwrap_or("")
}

fn public_avatar(raw: Option<&str>) -> String {
    let Ok(mut url) = Url::parse(text(raw, 2048)) else {
        return String::new();
    };
    if !url
        .host_str()
        .is_some_and(|host| host.ends_with(".music.126.net") || host.ends_with(".music.163.com"))
        || !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return String::new();
    }
    url.set_query(None);
    url.set_fragment(None);
    url.into()
}

fn project_reply(reply: Value, history: &[Entry]) -> Result<QuickLoginEligibilityView> {
    // A legacy whitelist has the opposite meaning from a JSK invalidation
    // list. Never treat a legacy-shaped 200 as "no accounts invalidated".
    if reply["code"] != 200 || reply.get("usableUserIds").is_some() {
        return Err(Error::Response("quick login eligibility was not confirmed"));
    }
    let mut invalid = BTreeSet::new();
    if let Some(values) = reply.get("deleteUserIds") {
        let values = values
            .as_array()
            .filter(|values| values.len() <= history.len())
            .ok_or(Error::Response("invalid quick login eligibility list"))?;
        for value in values {
            let uid = identifier(value)
                .map_err(|_| Error::Response("invalid quick login eligibility identity"))?;
            if !history.iter().any(|entry| entry.saved.uid == uid) || !invalid.insert(uid) {
                return Err(Error::Response("invalid quick login eligibility identity"));
            }
        }
    }
    Ok(QuickLoginEligibilityView {
        checked_count: history.len(),
        accounts: history
            .iter()
            .map(|entry| QuickLoginAccountView {
                id: entry.saved.session_id.clone(),
                current: entry.current,
                eligible: !invalid.contains(&entry.saved.uid),
            })
            .collect(),
    })
}
