//! Guest profile relationship, distinct from an artist subscription.
use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    account::SessionToken,
    error::code_matches,
    protocol::api::identifier,
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const PENDING: &str = "profile-follow-pending-v1";
const READ_CODES: &[i64] = &[200, 403, 404];

#[derive(Clone, Copy, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfileFollowCommand {
    Change { expected: bool },
    Reconcile,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileFollowStatus {
    Ready,
    Followed,
    Unfollowed,
    Unchanged,
    Uncertain,
    Limit,
    Privacy,
    Blocked,
    Rejected,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileFollowRelation {
    pub followed: bool,
    pub mutual: bool,
    pub followers_count: Option<u64>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileFollowView {
    pub relation: Option<ProfileFollowRelation>,
    pub pending: bool,
    pub status: ProfileFollowStatus,
}

pub(super) fn project(viewer: &str, uid: &str, profile: &Value) -> Option<ProfileFollowRelation> {
    let status = profile["accountStatus"]
        .as_i64()
        .or_else(|| profile["accountStatus"].as_str()?.parse().ok());
    if viewer == uid
        || profile["blacklist"] == true
        || profile["inBlacklist"] == true
        || matches!(status, Some(30 | 40))
    {
        return None;
    }
    let followed = profile["followed"].as_bool()?;
    Some(ProfileFollowRelation {
        followed,
        mutual: followed && profile["mutual"] == true,
        followers_count: profile["followeds"]
            .as_u64()
            .filter(|count| *count <= 9_007_199_254_740_991),
    })
}

fn view(
    relation: Option<ProfileFollowRelation>,
    pending: bool,
    status: ProfileFollowStatus,
) -> ProfileFollowView {
    ProfileFollowView {
        relation,
        pending,
        status,
    }
}

pub(super) fn pending(store: &StateStore, owner: &str, uid: &str) -> Result<Option<bool>> {
    store
        .get(owner, PENDING, uid)?
        .map(|value| {
            value["desired"]
                .as_bool()
                .ok_or(Error::State("invalid profile follow intent"))
        })
        .transpose()
}

fn denied(error: &Error) -> bool {
    error.is_service_code(403)
        || error.is_service_code(404)
        || matches!(
            error.root(),
            Error::HttpStatus {
                status: 403 | 404,
                ..
            }
        )
}

fn rejected(error: &Error) -> Option<ProfileFollowStatus> {
    [301, 315, 316, 320, 400, 403, 404]
        .into_iter()
        .find(|code| error.is_service_code(*code))
        .map(|code| match code {
            315 => ProfileFollowStatus::Privacy,
            316 => ProfileFollowStatus::Blocked,
            320 => ProfileFollowStatus::Limit,
            _ => ProfileFollowStatus::Rejected,
        })
}

impl MusicClient {
    pub(super) fn profile_follow_header(
        &self,
        token: &SessionToken,
        viewer: &str,
        uid: &str,
        profile: &Value,
    ) -> Result<Option<ProfileFollowView>> {
        let Some(relation) = project(viewer, uid, profile) else {
            return Ok(None);
        };
        let store = self.session.persistent_store()?;
        self.session.with_token(token, |owner| {
            let unresolved = pending(&store, owner, uid)?.is_some();
            Ok(Some(view(
                Some(relation),
                unresolved,
                if unresolved {
                    ProfileFollowStatus::Uncertain
                } else {
                    ProfileFollowStatus::Ready
                },
            )))
        })
    }

    async fn read_profile_follow(
        &self,
        token: &SessionToken,
        uid: &str,
    ) -> Result<Option<ProfileFollowRelation>> {
        self.session
            .run_for(token, |api, viewer| async move {
                let detail = match api
                    .call(
                        &format!("/api/w/v1/user/detail/{uid}"),
                        json!({"all":true,"userId":uid}),
                        false,
                        READ_CODES,
                    )
                    .await
                {
                    Ok(response) if code_matches(&response["code"], 200) => response,
                    Ok(_) => return Ok(None),
                    Err(error) if denied(&error) => return Ok(None),
                    Err(error) => return Err(error),
                };
                let profile = &detail["profile"];
                if identifier(&profile["userId"]).ok().as_deref() != Some(uid) {
                    return Err(Error::Response("profile follow identity mismatch"));
                }
                Ok(project(&viewer, uid, profile))
            })
            .await
    }

    pub async fn profile_follow(
        &self,
        epoch: &str,
        uid: &str,
        command: ProfileFollowCommand,
    ) -> Result<AccountData<ProfileFollowView>> {
        let token = self.playlist_token(epoch)?;
        let uid = identifier(&json!(uid))?;
        let owner = self.session.with_token(&token, |owner| {
            if owner == uid {
                return Err(Error::InvalidInput("cannot follow own profile"));
            }
            Ok(owner.to_owned())
        })?;
        let store = self.session.persistent_store()?;
        let _lock =
            OperationLock::acquire(store.directory(), &owner, &format!("profile-follow:{uid}"))?;
        let existing = pending(&store, &owner, &uid)?;
        if matches!(command, ProfileFollowCommand::Change { .. })
            && super::profile_blacklist::pending_for(&store, &owner, &uid)?
        {
            return Err(Error::MutationUncertain);
        }
        let value = match command {
            ProfileFollowCommand::Change { expected } if existing.is_none() => {
                let before = self.read_profile_follow(&token, &uid).await?;
                match before {
                    None => view(None, false, ProfileFollowStatus::Blocked),
                    Some(relation) if relation.followed != expected => {
                        view(before, false, ProfileFollowStatus::Unchanged)
                    }
                    Some(_) => {
                        let desired = !expected;
                        // Record before dispatch. A lost reply, process exit or
                        // account switch must never turn this into a retry.
                        self.session.with_token(&token, |owner| {
                            store.put(owner, PENDING, &uid, &json!({"desired":desired}))
                        })?;
                        let path = format!(
                            "/api/user/{}/{uid}",
                            if desired { "follow" } else { "delfollow" }
                        );
                        let accepted: &[i64] = if desired { &[200, 201] } else { &[200] };
                        let response = self
                            .session
                            .run_for(&token, |api, _| async move {
                                api.call(&path, json!({}), false, accepted).await
                            })
                            .await;
                        if let Err(error) = &response
                            && let Some(status) = rejected(error)
                        {
                            // This classification depends on explicit service rejection.
                            store.delete(&owner, PENDING, &uid)?;
                            let relation = if matches!(status, ProfileFollowStatus::Blocked) {
                                None
                            } else {
                                before
                            };
                            view(relation, false, status)
                        } else {
                            self.settle_profile_follow(&token, &store, &uid, desired, before)
                                .await?
                        }
                    }
                }
            }
            ProfileFollowCommand::Change { .. } => return Err(Error::MutationUncertain),
            ProfileFollowCommand::Reconcile => match existing {
                Some(desired) => {
                    self.settle_profile_follow(&token, &store, &uid, desired, None)
                        .await?
                }
                None => view(
                    self.read_profile_follow(&token, &uid).await?,
                    false,
                    ProfileFollowStatus::Unchanged,
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

    async fn settle_profile_follow(
        &self,
        token: &SessionToken,
        store: &StateStore,
        uid: &str,
        desired: bool,
        previous: Option<ProfileFollowRelation>,
    ) -> Result<ProfileFollowView> {
        let observed = match self.read_profile_follow(token, uid).await {
            Ok(result) => result,
            Err(error)
                if matches!(error.root(), Error::StaleResponse) || error.is_service_code(301) =>
            {
                return Err(error);
            }
            Err(_) => return Ok(view(previous, true, ProfileFollowStatus::Uncertain)),
        };
        if observed.is_some_and(|relation| relation.followed == desired) {
            self.session
                .with_token(token, |owner| store.delete(owner, PENDING, uid))?;
            return Ok(view(
                observed,
                false,
                if desired {
                    ProfileFollowStatus::Followed
                } else {
                    ProfileFollowStatus::Unfollowed
                },
            ));
        }
        Ok(view(observed, true, ProfileFollowStatus::Uncertain))
    }
}
