use super::{MutationPhase, MutationRecord, ResourceKind, record, snapshot};
use crate::{
    Error, Result,
    account::{
        AccountApi, AccountSession, LikeContext, SessionStatus, SessionToken,
        settings::{AccountSettings, SettingChange, SettingKey, compatibility_record},
        validate_settings,
    },
    protocol::api::identifier,
    storage::{OperationLock, StateStore},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) const JOURNAL: &str = "resource-mutations";
pub struct AccountResources {
    pub(super) session: Arc<AccountSession>,
    pub(super) store: Arc<StateStore>,
}

pub(super) fn now() -> Result<f64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .map_err(|_| Error::State("system clock precedes UNIX epoch"))
}

impl AccountResources {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        let store = session.persistent_store()?;
        Ok(Self { session, store })
    }
    fn token(&self) -> Result<SessionToken> {
        let snapshot = self.session.snapshot()?;
        if snapshot.status != SessionStatus::Authenticated || snapshot.token.uid.is_none() {
            return Err(Error::service("session", json!(301), Value::Null));
        }
        Ok(snapshot.token)
    }
    /// Local observation for native hosts. Unknown cache is not an unliked state.
    pub fn membership_for(
        &self,
        token: &SessionToken,
        kind: ResourceKind,
        id: &str,
    ) -> Result<(Option<bool>, Option<MutationRecord>)> {
        if kind == ResourceKind::Settings {
            return Err(Error::InvalidInput("settings are not memberships"));
        }
        let id = identifier(&json!(id))?;
        self.session.with_token(token, |uid| {
            let observed = self
                .store
                .get(uid, "resources", kind.as_str())?
                .map(|value| {
                    snapshot::value(kind, &id, &value).and_then(|value| {
                        value
                            .as_bool()
                            .ok_or(Error::Response("invalid membership observation"))
                    })
                })
                .transpose()?;
            let operation = self.load(uid, &record::key(kind, &id))?;
            Ok((observed, operation))
        })
    }
    pub async fn refresh_for(&self, token: &SessionToken, kind: ResourceKind) -> Result<Value> {
        if kind == ResourceKind::Settings {
            return Ok(AccountSettings::new(self.session.clone())?
                .refresh_for(token)
                .await?
                .values);
        }
        let result = self
            .session
            .run_for(token, |api, uid| async move {
                snapshot::read(api, &uid, kind).await
            })
            .await?;
        self.session
            .save_result(token, "resources", kind.as_str(), &result)?;
        Ok(result)
    }
    pub async fn refresh(&self, kind: ResourceKind) -> Result<Value> {
        self.refresh_for(&self.token()?, kind).await
    }
    pub fn operations(&self) -> Result<BTreeMap<String, MutationRecord>> {
        let token = self.token()?;
        self.operations_for(&token)
    }
    /// Domain-local journal inspection must not migrate or depend on account
    /// settings. Unrelated damaged settings cannot hide a pending song write.
    pub fn liked_operations_for(&self, token: &SessionToken) -> Result<Vec<MutationRecord>> {
        self.session.with_token(token, |uid| {
            self.store
                .values(uid, JOURNAL)?
                .into_iter()
                .filter(|(key, _)| key.starts_with("liked:"))
                .map(|(key, value)| {
                    let record: MutationRecord = serde_json::from_value(value)?;
                    if record.kind != ResourceKind::Liked
                        || record::key(record.kind, &record.key) != key
                        || !record.created_at.is_finite()
                    {
                        return Err(Error::Response("invalid like journal"));
                    }
                    identifier(&json!(record.key))?;
                    Ok(record)
                })
                .collect()
        })
    }
    pub fn operations_for(&self, token: &SessionToken) -> Result<BTreeMap<String, MutationRecord>> {
        let mut records: BTreeMap<String, MutationRecord> =
            self.session.with_token(token, |uid| {
                self.store
                    .values(uid, JOURNAL)?
                    .into_iter()
                    .map(|(key, value)| Ok((key, serde_json::from_value(value)?)))
                    .collect()
            })?;
        for record in AccountSettings::new(self.session.clone())?.compatibility_operations(token)? {
            records.insert(record::key(record.kind, &record.key), record);
        }
        Ok(records)
    }
    pub async fn set_membership(
        &self,
        kind: ResourceKind,
        id: &Value,
        desired: bool,
    ) -> Result<MutationRecord> {
        self.set_membership_for(&self.token()?, kind, id, desired)
            .await
    }
    /// Bind a host action to the account which displayed it. A delayed UI
    /// action must never be silently applied to a newly selected account.
    pub async fn set_membership_for(
        &self,
        expected: &SessionToken,
        kind: ResourceKind,
        id: &Value,
        desired: bool,
    ) -> Result<MutationRecord> {
        if kind == ResourceKind::Settings {
            return Err(Error::InvalidInput(
                "settings are not collection memberships",
            ));
        }
        self.change_for(expected, kind, &identifier(id)?, json!(desired))
            .await
    }
    pub async fn set_setting(&self, key: &str, value: Value) -> Result<MutationRecord> {
        validate_settings(&json!({key:value}))?;
        let key = SettingKey::parse(key)?;
        let value = key
            .value(&value)
            .ok_or(Error::InvalidInput("invalid setting value"))?;
        let token = self.token()?;
        let service = AccountSettings::new(self.session.clone())?;
        let snapshot = service.refresh_for(&token).await?;
        let op = service
            .change_for(
                &token,
                &uuid::Uuid::new_v4().to_string(),
                &snapshot.revision,
                SettingChange { key, value },
            )
            .await?;
        if op.phase.unresolved() {
            return Err(Error::MutationUncertain);
        }
        compatibility_record(op)
    }
    pub(super) fn load(&self, uid: &str, key: &str) -> Result<Option<MutationRecord>> {
        let record: Option<MutationRecord> = self
            .store
            .get(uid, JOURNAL, key)?
            .map(serde_json::from_value)
            .transpose()?;
        if record
            .as_ref()
            .is_some_and(|r| record::key(r.kind, &r.key) != key || !r.created_at.is_finite())
        {
            return Err(Error::Response("invalid resource operation journal"));
        }
        Ok(record)
    }
    async fn change_for(
        &self,
        token: &SessionToken,
        kind: ResourceKind,
        key: &str,
        desired: Value,
    ) -> Result<MutationRecord> {
        self.session.with_token(token, |_| Ok(()))?;
        let uid = token
            .uid
            .as_deref()
            .ok_or(Error::State("missing account"))?;
        let journal_key = record::key(kind, key);
        let _lock = OperationLock::acquire(self.store.directory(), uid, &journal_key)?;
        if self
            .load(uid, &journal_key)?
            .is_some_and(|r| r.phase.pending())
        {
            return Err(Error::MutationUncertain);
        }
        let snapshot = self.refresh_for(token, kind).await?;
        let before = snapshot::value(kind, key, &snapshot)?;
        if !snapshot::same_type(&before, &desired) {
            return Err(Error::Response("server returned an unexpected state type"));
        }
        let mut record = MutationRecord {
            kind,
            key: key.into(),
            before,
            desired,
            created_at: now()?,
            phase: MutationPhase::Sending,
            observed: None,
            checked_at: None,
            service_code: None,
            error_class: None,
        };
        if record.before == record.desired {
            record.phase = MutationPhase::Unchanged;
        }
        self.session.save_result(
            token,
            JOURNAL,
            &journal_key,
            &serde_json::to_value(&record)?,
        )?;
        if record.phase == MutationPhase::Unchanged {
            return Ok(record);
        }
        let desired = record.desired.clone();
        let key = key.to_owned();
        let result = self
            .session
            .run_for(token, |api, uid| async move {
                if kind == ResourceKind::Mvs {
                    return crate::mv::MvApi::new(api)
                        .collect(
                            &json!(key),
                            desired
                                .as_bool()
                                .ok_or(Error::State("invalid desired membership"))?,
                        )
                        .await;
                }
                let api = AccountApi::new(api);
                match kind {
                    ResourceKind::Liked => {
                        api.set_like(
                            &json!(uid),
                            &json!(key),
                            desired
                                .as_bool()
                                .ok_or(Error::State("invalid desired membership"))?,
                            LikeContext::default(),
                        )
                        .await
                    }
                    ResourceKind::Albums => {
                        api.collect_album(
                            &json!(key),
                            desired
                                .as_bool()
                                .ok_or(Error::State("invalid desired membership"))?,
                        )
                        .await
                    }
                    ResourceKind::Artists => {
                        api.collect_artist(
                            &json!(key),
                            desired
                                .as_bool()
                                .ok_or(Error::State("invalid desired membership"))?,
                        )
                        .await
                    }
                    ResourceKind::Settings => {
                        Err(Error::State("settings require their coordinator"))
                    }
                    ResourceKind::Mvs => unreachable!("handled before AccountApi"),
                }
            })
            .await;
        match result {
            Ok(response) => {
                record.phase = MutationPhase::Verifying;
                record.service_code = response.get("code").cloned();
            }
            Err(error) => {
                record.phase = if [301, 403, 505, 506, 511, 512]
                    .iter()
                    .any(|code| error.is_service_code(*code))
                {
                    MutationPhase::Rejected
                } else {
                    MutationPhase::Uncertain
                };
                record.error_class = Some(
                    match &error {
                        Error::Service { .. } | Error::HttpStatus { .. } => "service",
                        Error::StaleResponse => "stale_response",
                        Error::Transport(_) => "transport",
                        _ => "state_or_response",
                    }
                    .into(),
                );
                record.service_code = match &error {
                    Error::Service { code, .. } => Some(code.clone()),
                    Error::HttpStatus {
                        body: Some(body), ..
                    } => body.get("code").cloned(),
                    _ => None,
                };
                // Keep recovery responsibility in the ORIGINAL account even
                // after switching away. Never publish this into the new UID.
                self.store
                    .put(uid, JOURNAL, &journal_key, &serde_json::to_value(record)?)?;
                return Err(error);
            }
        }
        self.store
            .put(uid, JOURNAL, &journal_key, &serde_json::to_value(&record)?)?;
        self.reconcile_for(token, &journal_key, record).await
    }
    pub async fn reconcile(&self, kind: ResourceKind, key: &str) -> Result<MutationRecord> {
        self.reconcile_expected(&self.token()?, kind, key).await
    }
    pub async fn reconcile_expected(
        &self,
        token: &SessionToken,
        kind: ResourceKind,
        key: &str,
    ) -> Result<MutationRecord> {
        if kind == ResourceKind::Settings {
            return AccountSettings::new(self.session.clone())?
                .reconcile_compatibility(token, key)
                .await;
        }
        self.session.with_token(token, |_| Ok(()))?;
        let key = identifier(&json!(key))?;
        let uid = token
            .uid
            .as_deref()
            .ok_or(Error::State("missing account"))?;
        let journal_key = record::key(kind, &key);
        let _lock = OperationLock::acquire(self.store.directory(), uid, &journal_key)?;
        let record = self
            .load(uid, &journal_key)?
            .ok_or(Error::State("no operation to reconcile"))?;
        if !record.phase.pending() {
            self.session.with_token(token, |_| Ok(()))?;
            return Ok(record);
        }
        self.reconcile_for(token, &journal_key, record).await
    }
    pub(super) async fn reconcile_for(
        &self,
        token: &SessionToken,
        key: &str,
        mut record: MutationRecord,
    ) -> Result<MutationRecord> {
        let snapshot = self.refresh_for(token, record.kind).await?;
        let actual = snapshot::value(record.kind, &record.key, &snapshot)?;
        let matched = snapshot::same_type(&actual, &record.desired) && actual == record.desired;
        record.observed = Some(actual);
        record.checked_at = Some(now()?);
        record.phase = if matched {
            MutationPhase::Confirmed
        } else {
            MutationPhase::Uncertain
        };
        self.session
            .save_result(token, JOURNAL, key, &serde_json::to_value(&record)?)?;
        if !matched {
            return Err(Error::MutationUncertain);
        }
        Ok(record)
    }
}
