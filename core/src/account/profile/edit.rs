//! Durable profile updates. A write acknowledgement never replaces readback.
mod patch;

use super::{AccountProfile, SelfProfileView};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{OperationLock, StateStore, StateTransaction},
};
pub use patch::{ProfileBirthday, ProfilePatch, ProfileRegion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;

const NAMESPACE: &str = "account-profile-edit-v1";
const JOURNAL: &str = "account-profile-edit-intents-v1";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileEditSnapshot {
    pub revision: String,
    pub fetched_at: u64,
    pub profile: SelfProfileView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileEditPhase {
    Pending,
    Confirmed,
    Rejected,
    Uncertain,
    Unchanged,
    AcceptedCurrentState,
}

impl ProfileEditPhase {
    pub fn unresolved(self) -> bool {
        matches!(self, Self::Pending | Self::Uncertain)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileEditOperation {
    pub id: String,
    pub expected_revision: String,
    pub patch: ProfilePatch,
    pub submitted: ProfilePatch,
    pub phase: ProfileEditPhase,
    pub created_at: u64,
    pub observed: Option<ProfileEditSnapshot>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileEditPage {
    pub snapshot: Option<ProfileEditSnapshot>,
    pub operations: Vec<ProfileEditOperation>,
}

pub struct ProfileEditor {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
}

fn now() -> u64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0) as u64
}

fn revision(profile: &SelfProfileView) -> Result<String> {
    let editable = (
        &profile.uid,
        &profile.nickname,
        &profile.signature,
        profile.gender,
        profile.birthday,
        profile.province,
        profile.city,
    );
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&editable)?)))
}

fn validate_id(id: &str) -> Result<()> {
    if !uuid::Uuid::parse_str(id).is_ok_and(|value| value.hyphenated().to_string() == id) {
        return Err(Error::InvalidInput(
            "profile intent requires a canonical UUID",
        ));
    }
    Ok(())
}

fn validate_revision(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::InvalidInput("invalid profile revision"));
    }
    Ok(())
}

fn preflight_error(error: Error) -> Error {
    if error.is_service_code(301)
        || matches!(
            error.root(),
            Error::StaleResponse
                | Error::RequestNotSent(_)
                | Error::Response("nickname availability is unknown")
                | Error::InvalidInput(
                    "profile nickname is already in use" | "profile nickname is prohibited"
                )
        )
    {
        return error;
    }
    Error::RequestNotSent(Box::new(error))
}

fn check_snapshot(uid: &str, snapshot: &ProfileEditSnapshot) -> Result<()> {
    if snapshot.profile.uid != uid || snapshot.revision != revision(&snapshot.profile)? {
        return Err(Error::State("invalid profile edit snapshot"));
    }
    Ok(())
}

fn store_snapshot(tx: &mut StateTransaction<'_>, snapshot: &ProfileEditSnapshot) -> Result<()> {
    tx.put(
        &snapshot.profile.uid,
        NAMESPACE,
        "snapshot",
        &serde_json::to_value(snapshot)?,
    )
}

fn store_operation(
    tx: &mut StateTransaction<'_>,
    uid: &str,
    op: &ProfileEditOperation,
) -> Result<()> {
    tx.put(uid, JOURNAL, &op.id, &serde_json::to_value(op)?)
}

impl ProfileEditor {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }

    fn lock(&self, token: &SessionToken) -> Result<(String, OperationLock)> {
        let uid = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let guard = OperationLock::acquire(self.store.directory(), NAMESPACE, &uid)?;
        Ok((uid, guard))
    }

    fn stored_page(&self, token: &SessionToken) -> Result<ProfileEditPage> {
        self.session.with_token(token, |uid| {
            self.store.transaction(|tx| {
                let snapshot = tx
                    .get(uid, NAMESPACE, "snapshot")?
                    .map(serde_json::from_value::<ProfileEditSnapshot>)
                    .transpose()?;
                if let Some(value) = &snapshot {
                    check_snapshot(uid, value)?;
                }
                let operations = tx
                    .values(uid, JOURNAL)?
                    .into_iter()
                    .map(|(key, value)| {
                        let op: ProfileEditOperation = serde_json::from_value(value)?;
                        validate_id(&key)?;
                        validate_revision(&op.expected_revision)?;
                        if key != op.id
                            || op.patch.clone().normalized()? != op.patch
                            || op.submitted.clone().normalized()? != op.submitted
                        {
                            return Err(Error::State("invalid profile edit journal"));
                        }
                        if let Some(observed) = &op.observed {
                            check_snapshot(uid, observed)?;
                        }
                        Ok(op)
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(ProfileEditPage {
                    snapshot,
                    operations,
                })
            })
        })
    }

    pub fn page_for(&self, token: &SessionToken) -> Result<ProfileEditPage> {
        let mut page = self.stored_page(token)?;
        page.operations.sort_by_key(|op| {
            (
                std::cmp::Reverse(op.phase.unresolved()),
                std::cmp::Reverse(op.created_at),
            )
        });
        page.operations.truncate(20);
        Ok(page)
    }

    async fn remote(&self, token: &SessionToken) -> Result<ProfileEditSnapshot> {
        let profile = AccountProfile::new(self.session.clone())
            .detail_for(token)
            .await?;
        Ok(ProfileEditSnapshot {
            revision: revision(&profile)?,
            fetched_at: now(),
            profile,
        })
    }

    fn remember(&self, token: &SessionToken, snapshot: &ProfileEditSnapshot) -> Result<()> {
        self.session.with_token(token, |_| {
            self.store.transaction(|tx| store_snapshot(tx, snapshot))
        })
    }

    pub async fn refresh_for(&self, token: &SessionToken) -> Result<ProfileEditPage> {
        let (_uid, _guard) = self.lock(token)?;
        self.remember(token, &self.remote(token).await?)?;
        self.page_for(token)
    }

    pub async fn change_for(
        &self,
        token: &SessionToken,
        id: &str,
        expected_revision: &str,
        patch: ProfilePatch,
    ) -> Result<ProfileEditOperation> {
        validate_id(id)?;
        validate_revision(expected_revision)?;
        let patch = patch.normalized()?;
        let (uid, _guard) = self.lock(token)?;
        let page = self.stored_page(token)?;
        if let Some(op) = page.operations.iter().find(|op| op.id == id) {
            if op.expected_revision != expected_revision || op.patch != patch {
                return Err(Error::InvalidInput("profile intent ID reused"));
            }
            return Ok(op.clone());
        }
        if page.operations.iter().any(|op| op.phase.unresolved()) {
            return Err(Error::MutationUncertain);
        }
        patch.validate_today()?;
        if page.snapshot.as_ref().map(|value| value.revision.as_str()) != Some(expected_revision) {
            return Err(Error::State("read current profile before editing"));
        }
        let before = self.remote(token).await.map_err(preflight_error)?;
        self.remember(token, &before)?;
        if before.revision != expected_revision {
            return Err(Error::State("account profile changed"));
        }
        let submitted = patch.changed_from(&before.profile)?;
        let mut op = ProfileEditOperation {
            id: id.into(),
            expected_revision: expected_revision.into(),
            patch,
            submitted,
            phase: ProfileEditPhase::Pending,
            created_at: now(),
            observed: None,
        };
        if op.submitted.is_empty() {
            op.phase = ProfileEditPhase::Unchanged;
            op.observed = Some(before);
        } else if let Some(name) = &op.submitted.nickname {
            self.check_nickname(token, name.clone())
                .await
                .map_err(preflight_error)?;
        }
        let wire = op.submitted.wire()?;
        self.session.with_token(token, |_| {
            self.store.transaction(|tx| store_operation(tx, &uid, &op))
        })?;
        if op.phase == ProfileEditPhase::Unchanged {
            return Ok(op);
        }
        let result = self
            .session
            .run_for(token, |api, _| async move {
                // M15:3649; M3101:917–941. Numeric and string code 200 acknowledge,
                // but neither alone establishes what the server applied.
                api.call("/api/user/profile/update", wire, false, &[200])
                    .await
            })
            .await;
        match result {
            Ok(_) => match self.observe(token, op.clone(), None).await {
                Ok(observed) => return Ok(observed),
                Err(_) => op.phase = ProfileEditPhase::Uncertain,
            },
            Err(error) => {
                op.phase = if matches!(error.root(), Error::RequestNotSent(_)) {
                    ProfileEditPhase::Rejected
                } else {
                    // No endpoint-specific terminal failure codes were recovered.
                    ProfileEditPhase::Uncertain
                }
            }
        }
        // Preserve responsibility for the original owner if the session changed.
        self.store
            .transaction(|tx| store_operation(tx, &uid, &op))?;
        self.session.with_token(token, |_| Ok(op))
    }

    async fn check_nickname(&self, token: &SessionToken, name: String) -> Result<()> {
        let result = self
            .session
            .run_for(token, |api, _| async move {
                let value = api
                    .call(
                        "/api/nickname/duplicated",
                        json!({"nickname":name}),
                        false,
                        &[200],
                    )
                    .await?;
                match value["duplicated"].as_bool() {
                    Some(false) => Ok(()),
                    Some(true) => Err(Error::InvalidInput("profile nickname is already in use")),
                    None => Err(Error::Response("nickname availability is unknown")),
                }
            })
            .await;
        match result {
            Err(error) if error.is_service_code(407) => {
                Err(Error::InvalidInput("profile nickname is prohibited"))
            }
            other => other,
        }
    }

    fn operation(&self, token: &SessionToken, id: &str) -> Result<ProfileEditOperation> {
        self.stored_page(token)?
            .operations
            .into_iter()
            .find(|op| op.id == id)
            .ok_or(Error::State("no profile edit operation to reconcile"))
    }

    pub async fn reconcile_for(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<ProfileEditOperation> {
        validate_id(id)?;
        let (_uid, _guard) = self.lock(token)?;
        let op = self.operation(token, id)?;
        if !op.phase.unresolved() {
            return Ok(op);
        }
        self.observe(token, op, None).await
    }

    pub async fn accept_current_for(
        &self,
        token: &SessionToken,
        id: &str,
        expected_revision: &str,
    ) -> Result<ProfileEditOperation> {
        validate_id(id)?;
        validate_revision(expected_revision)?;
        let (_uid, _guard) = self.lock(token)?;
        let op = self.operation(token, id)?;
        if !op.phase.unresolved() {
            return Ok(op);
        }
        if op
            .observed
            .as_ref()
            .map(|snapshot| snapshot.revision.as_str())
            != Some(expected_revision)
        {
            return Err(Error::State("reconcile profile edit before acceptance"));
        }
        self.observe(token, op, Some(expected_revision)).await
    }

    async fn observe(
        &self,
        token: &SessionToken,
        mut op: ProfileEditOperation,
        accept_revision: Option<&str>,
    ) -> Result<ProfileEditOperation> {
        let api = self.session.native_api(token)?;
        let snapshot = self.remote(token).await?;
        let accept = accept_revision == Some(snapshot.revision.as_str());
        let confirmed = op.submitted.changed_from(&snapshot.profile)?.is_empty();
        op.phase = if confirmed {
            ProfileEditPhase::Confirmed
        } else if accept {
            ProfileEditPhase::AcceptedCurrentState
        } else {
            ProfileEditPhase::Uncertain
        };
        op.observed = Some(snapshot.clone());
        let persist = |tx: &mut StateTransaction<'_>, uid: &str| {
            store_snapshot(tx, &snapshot)?;
            store_operation(tx, uid, &op)
        };
        if confirmed || accept {
            self.session
                .commit_profile_edit(token, &api, &snapshot.profile, persist)?;
        } else {
            self.session
                .with_token(token, |uid| self.store.transaction(|tx| persist(tx, uid)))?;
        }
        if accept_revision.is_some() && !accept && !confirmed {
            return Err(Error::State("account profile changed"));
        }
        Ok(op)
    }
}
