//! Explicit JSK eligibility check. It never issues a login or changes profiles.
mod exchange;
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
    current: bool,
}

struct History {
    selection: Option<Selection>,
    entries: Vec<Entry>,
}

pub(super) struct QuickSavedTarget {
    pub(super) saved: SavedProfile,
    credential_revision: String,
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
        let history = {
            let (registry, target) = (self.clone(), target.clone());
            tokio::task::spawn_blocking(move || {
                registry.check_quick_history(&history, &target)?;
                Ok::<_, Error>(history)
            })
            .await
            .map_err(|_| Error::State("quick history worker stopped"))??
        };
        session.require_api(expected, &api)?;
        if native.credentials().request_credentials(&target)?.revision != revision {
            return Err(Error::CredentialChanged);
        }
        Ok((history, view))
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
                        Ok(saved)
                    })();
                    match parsed {
                        Err(Error::State("saved account removal has started")) => None,
                        other => Some(other),
                    }
                })
                .collect::<Result<Vec<_>>>()
        })?;
        let mut entries = Vec::new();
        let mut session_ids = BTreeSet::new();
        for saved in rows {
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
                });
            }
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
        let rows = history.entries.iter().map(|entry| {
            let saved = &entry.saved;
            let raw = self.store.get(&saved.uid, "account", "profile")?.unwrap_or(Value::Null);
            if !raw.is_null() && identifier(&raw["userId"]).as_deref().ok() != Some(saved.uid.as_str()) {
                return Err(Error::State("saved quick login profile identity mismatch"));
            }
            // M290:524–542 derives exactly these userProfile fields; never
            // copy a raw account/get response or any bindings into the list.
            let image_id = &raw["avatarImgId"];
            let image_id = if identifier(image_id).is_ok_and(|id| id.len() <= 32) { image_id.clone() } else { json!(0) };
            let phone = text(raw["cellphone"].as_str(), 32);
            let phone = if phone.bytes().all(|b| b.is_ascii_digit()) { phone } else { "" };
            Ok(json!({
                "userId": saved.uid,
                "userProfile": {
                    "userId": saved.uid,
                    "nickname": text(Some(&saved.nickname), 512),
                    "avatarUrl": public_avatar(saved.avatar_url.as_deref()),
                    "avatarImgId": image_id,
                    "userType": raw["userType"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
                    "cellphone": phone,
                },
                "musicU": entry.credential.music_u,
            }))
        }).collect::<Result<Vec<_>>>()?;
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
        if !Credentials::quick_login_credential(&database, self.origin(), &target)?
            .is_some_and(|credential| credential.revision == binding.credential_revision)
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
