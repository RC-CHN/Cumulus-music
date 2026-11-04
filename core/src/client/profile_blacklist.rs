//! Official PC user blacklist; independent of music, artist and FM suppression.
use super::{MusicClient, dto::AccountData, mapping};
use crate::{
    Error, Result,
    account::SessionToken,
    error::code_matches,
    protocol::api::identifier,
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

const LIST: &str = "/api/blacklist/get";
const ADD: &str = "/api/blacklist/add";
const DELETE: &str = "/api/blacklist/delete";
const JOURNAL: &str = "profile-blacklist-pending-v1";
const PENDING: &str = "current";
const MAX_USERS: usize = 1000;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileBlacklistOrigin {
    Profile,
    List,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfileBlacklistCommand {
    Change {
        expected: bool,
        origin: ProfileBlacklistOrigin,
    },
    Reconcile,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileBlacklistStatus {
    Added,
    Removed,
    Unchanged,
    AlreadyBlocked,
    Full,
    Unavailable,
    Rejected,
    Uncertain,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileBlacklistEntry {
    pub id: String,
    pub nickname: String,
    pub avatar_url: Option<String>,
    pub signature: String,
    pub closed: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileBlacklistHeader {
    pub blocked: bool,
    pub can_add: bool,
    pub pending: bool,
    pub busy_with_another: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileBlacklistPending {
    pub id: String,
    pub desired: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileBlacklistList {
    pub users: Vec<ProfileBlacklistEntry>,
    pub pending: Option<ProfileBlacklistPending>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileBlacklistResult {
    pub status: ProfileBlacklistStatus,
    pub blocked: Option<bool>,
    pub pending: Option<ProfileBlacklistPending>,
    pub users: Option<Vec<ProfileBlacklistEntry>>,
}

fn user_id(value: &Value) -> Result<String> {
    let id = identifier(value)?;
    if id.len() > 32 || id.starts_with('0') {
        return Err(Error::InvalidInput("invalid blacklist user identifier"));
    }
    Ok(id)
}

fn closed(profile: &Value) -> bool {
    let status = &profile["accountStatus"];
    matches!(
        status.as_i64().or_else(|| status.as_str()?.parse().ok()),
        Some(30 | 40)
    )
}

fn label(value: &Value, size: usize) -> String {
    value
        .as_str()
        .unwrap_or_default()
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(size)
        .collect()
}

fn project_list(value: &Value, owner: &str) -> Result<Vec<ProfileBlacklistEntry>> {
    // M2264 fixes more=false; do not treat an incomplete/invalid response
    // as proof that a particular user is absent.
    let rows = value["data"]
        .as_array()
        .filter(|rows| rows.len() <= MAX_USERS && value["hasMore"] != true && value["more"] != true)
        .ok_or(Error::Response("invalid user blacklist snapshot"))?;
    let mut seen = HashSet::with_capacity(rows.len());
    let mut output = Vec::with_capacity(rows.len());
    for row in rows {
        let id = user_id(&row["userId"])?;
        if !seen.insert(id.clone()) {
            return Err(Error::Response("duplicate user blacklist identifier"));
        }
        if id == owner {
            continue;
        }
        let nickname = ["nickname", "nickName", "userName"]
            .iter()
            .map(|field| label(&row[*field], 128))
            .find(|name| !name.is_empty())
            .unwrap_or_else(|| "网易云用户".into());
        output.push(ProfileBlacklistEntry {
            id,
            nickname,
            avatar_url: mapping::artwork(&row["avatarUrl"]),
            signature: label(&row["signature"], 256),
            closed: closed(row),
        });
    }
    Ok(output)
}

fn read_intent(store: &StateStore, owner: &str) -> Result<Option<ProfileBlacklistPending>> {
    store
        .get(owner, JOURNAL, PENDING)?
        .map(|raw| {
            let intent: ProfileBlacklistPending = serde_json::from_value(raw)?;
            user_id(&json!(intent.id))?;
            if intent.id == owner {
                return Err(Error::State("invalid user blacklist intent"));
            }
            Ok(intent)
        })
        .transpose()
}

pub(super) fn pending_for(store: &StateStore, owner: &str, id: &str) -> Result<bool> {
    Ok(read_intent(store, owner)?.is_some_and(|intent| intent.id == id))
}

fn has_user(rows: &[ProfileBlacklistEntry], uid: &str) -> bool {
    rows.iter().any(|entry| entry.id == uid)
}

fn result(
    status: ProfileBlacklistStatus,
    uid: &str,
    rows: Option<Vec<ProfileBlacklistEntry>>,
    pending: Option<ProfileBlacklistPending>,
) -> ProfileBlacklistResult {
    ProfileBlacklistResult {
        status,
        blocked: rows.as_ref().map(|rows| has_user(rows, uid)),
        pending,
        users: rows,
    }
}

impl MusicClient {
    pub(super) fn profile_blacklist_header(
        &self,
        token: &SessionToken,
        viewer: &str,
        target: &str,
        profile: &Value,
    ) -> Result<Option<ProfileBlacklistHeader>> {
        if token.uid.is_none() || viewer == target {
            return Ok(None);
        }
        let blocked = profile["blacklist"] == true;
        let can_add = !blocked && profile["inBlacklist"] != true && !closed(profile);
        let store = self.session.persistent_store()?;
        self.session.with_token(token, |owner| {
            let intent = read_intent(&store, owner)?;
            let pending = intent.as_ref().is_some_and(|intent| intent.id == target);
            Ok(
                (blocked || can_add || pending).then_some(ProfileBlacklistHeader {
                    blocked,
                    can_add,
                    pending,
                    busy_with_another: intent.is_some() && !pending,
                }),
            )
        })
    }

    async fn read_user_blacklist(
        &self,
        token: &SessionToken,
    ) -> Result<Vec<ProfileBlacklistEntry>> {
        if token.uid.is_none() {
            return Err(Error::State(
                "user blacklist requires an authenticated account",
            ));
        }
        self.session
            .run_for(token, |api, owner| async move {
                let reply = api
                    .call(LIST, json!({"userid":owner}), false, &[200])
                    .await?;
                project_list(&reply, &owner)
            })
            .await
    }

    pub async fn profile_blacklist_list(
        &self,
        epoch: &str,
    ) -> Result<AccountData<ProfileBlacklistList>> {
        let token = self.playlist_token(epoch)?;
        let users = self.read_user_blacklist(&token).await?;
        let store = self.session.persistent_store()?;
        self.session.with_token(&token, |owner| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: ProfileBlacklistList {
                    users,
                    pending: read_intent(&store, owner)?,
                },
            })
        })
    }

    pub async fn profile_blacklist(
        &self,
        epoch: &str,
        target: &str,
        command: ProfileBlacklistCommand,
    ) -> Result<AccountData<ProfileBlacklistResult>> {
        let token = self.playlist_token(epoch)?;
        let uid = user_id(&json!(target))?;
        let owner =
            token
                .uid
                .as_deref()
                .filter(|owner| *owner != uid)
                .ok_or(Error::InvalidInput(
                    "user blacklist requires another account",
                ))?;
        let store = self.session.persistent_store()?;
        let _blacklist = OperationLock::acquire(store.directory(), owner, "profile-blacklist")?;
        // Adding someone may also change follow state upstream. Serialize the
        // two controls for this target and never dispatch over a lost follow write.
        let _follow =
            OperationLock::acquire(store.directory(), owner, &format!("profile-follow:{uid}"))?;
        let existing = read_intent(&store, owner)?;
        let value = match command {
            ProfileBlacklistCommand::Change { expected, origin } => {
                if existing.is_some()
                    || super::profile_follow::pending(&store, owner, &uid)?.is_some()
                {
                    return Err(Error::MutationUncertain);
                }
                if !expected && matches!(origin, ProfileBlacklistOrigin::List) {
                    return Err(Error::InvalidInput("cannot add from the blacklist list"));
                }
                let before = self.read_user_blacklist(&token).await?;
                let desired = !expected;
                if has_user(&before, &uid) != expected {
                    result(ProfileBlacklistStatus::Unchanged, &uid, Some(before), None)
                } else if desired && !self.can_add_user_blacklist(&token, &uid).await? {
                    result(
                        ProfileBlacklistStatus::Unavailable,
                        &uid,
                        Some(before),
                        None,
                    )
                } else {
                    let intent = ProfileBlacklistPending {
                        id: uid.clone(),
                        desired,
                    };
                    self.session.with_token(&token, |owner| {
                        store.put(owner, JOURNAL, PENDING, &json!(intent))
                    })?;
                    let fence = &token;
                    let target = uid.as_str();
                    let response = self
                        .session
                        .run_for(fence, |api, _| async move {
                            self.session.require_api(fence, &api)?;
                            api.call(
                                if desired { ADD } else { DELETE },
                                json!({"userid":target}),
                                false,
                                if desired { &[200, -1, -2] } else { &[200] },
                            )
                            .await
                        })
                        .await;
                    match response {
                        Ok(reply) if desired && code_matches(&reply["code"], -2) => {
                            store.delete(owner, JOURNAL, PENDING)?;
                            result(ProfileBlacklistStatus::Full, &uid, Some(before), None)
                        }
                        Err(Error::RequestNotSent(_)) => {
                            store.delete(owner, JOURNAL, PENDING)?;
                            result(ProfileBlacklistStatus::Rejected, &uid, Some(before), None)
                        }
                        receipt => {
                            let duplicate = receipt
                                .as_ref()
                                .is_ok_and(|reply| desired && code_matches(&reply["code"], -1));
                            self.settle_user_blacklist(&token, &store, intent, duplicate)
                                .await?
                        }
                    }
                }
            }
            ProfileBlacklistCommand::Reconcile => match existing {
                Some(intent) if intent.id == uid => {
                    self.settle_user_blacklist(&token, &store, intent, false)
                        .await?
                }
                Some(_) => return Err(Error::MutationUncertain),
                None => result(
                    ProfileBlacklistStatus::Unchanged,
                    &uid,
                    Some(self.read_user_blacklist(&token).await?),
                    None,
                ),
            },
        };
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    async fn can_add_user_blacklist(&self, token: &SessionToken, uid: &str) -> Result<bool> {
        self.session
            .run_for(token, |api, _| async move {
                let reply = api
                    .call(
                        &format!("/api/w/v1/user/detail/{uid}"),
                        json!({"all":true,"userId":uid}),
                        false,
                        &[200, 403, 404],
                    )
                    .await?;
                if !code_matches(&reply["code"], 200) {
                    return Ok(false);
                }
                let profile = &reply["profile"];
                if user_id(&profile["userId"])? != uid {
                    return Err(Error::Response("blacklist target identity mismatch"));
                }
                // A contradictory list/detail is not an authorization to add.
                Ok(profile["blacklist"] != true
                    && profile["inBlacklist"] != true
                    && !closed(profile))
            })
            .await
    }

    async fn settle_user_blacklist(
        &self,
        token: &SessionToken,
        store: &StateStore,
        intent: ProfileBlacklistPending,
        duplicate: bool,
    ) -> Result<ProfileBlacklistResult> {
        let rows = match self.read_user_blacklist(token).await {
            Ok(rows) => Some(rows),
            Err(error)
                if error.is_service_code(301)
                    || matches!(
                        error.root(),
                        Error::StaleResponse | Error::CredentialChanged | Error::LoginDeferred
                    ) =>
            {
                return Err(error);
            }
            Err(_) => None,
        };
        let confirmed = rows
            .as_ref()
            .is_some_and(|rows| has_user(rows, &intent.id) == intent.desired);
        let status = if confirmed {
            self.session
                .with_token(token, |owner| store.delete(owner, JOURNAL, PENDING))?;
            if duplicate {
                ProfileBlacklistStatus::AlreadyBlocked
            } else if intent.desired {
                ProfileBlacklistStatus::Added
            } else {
                ProfileBlacklistStatus::Removed
            }
        } else {
            ProfileBlacklistStatus::Uncertain
        };
        let uid = intent.id.clone();
        Ok(result(status, &uid, rows, (!confirmed).then_some(intent)))
    }
}
