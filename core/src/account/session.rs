//! Account-generation and QR state machines recovered from the official host.
//! Network awaits never hold the state lock. QR authorization does not itself
//! authenticate an account; identity must still be read and committed.
mod invalidation;
mod legacy_api;
mod policy;
use super::login_policy::LoginNotice;
use crate::{
    Error, Result,
    protocol::api::{Api, identifier},
    storage::{StateStore, StateTransaction},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    future::Future,
    sync::{Arc, Mutex, MutexGuard},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionToken {
    pub generation: u64,
    pub uid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Unknown,
    Authenticated,
    Expired,
    SignedOut,
}

pub struct SessionSnapshot {
    pub token: SessionToken,
    pub status: SessionStatus,
    pub profile: Option<Value>,
    pub login_notice: Option<LoginNotice>,
}

struct SessionState {
    api: Arc<dyn Api>,
    generation: u64,
    uid: Option<String>,
    profile: Option<Value>,
    status: SessionStatus,
    login_notice: Option<LoginNotice>,
    changes: tokio::sync::watch::Sender<SessionToken>,
}

impl SessionState {
    fn token(&self) -> SessionToken {
        SessionToken {
            generation: self.generation,
            uid: self.uid.clone(),
        }
    }

    fn require(&self, token: &SessionToken) -> Result<()> {
        if self.token() != *token {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    fn invalidate(&mut self, status: SessionStatus) -> Result<()> {
        self.generation = next_generation(self.generation)?;
        self.uid = None;
        self.profile = None;
        self.status = status;
        if status != SessionStatus::Expired {
            self.login_notice = None;
        }
        self.changes.send_replace(self.token());
        Ok(())
    }
}

pub(super) fn next_generation(value: u64) -> Result<u64> {
    value
        .checked_add(1)
        .ok_or(Error::State("generation exhausted"))
}

pub struct AccountSession {
    state: Mutex<SessionState>,
    store: Option<Arc<StateStore>>,
    policy: policy::PolicyHost,
}

impl AccountSession {
    pub fn new(api: Arc<dyn Api>, store: Option<Arc<StateStore>>) -> Self {
        let (changes, _) = tokio::sync::watch::channel(SessionToken {
            generation: 0,
            uid: None,
        });
        Self {
            state: Mutex::new(SessionState {
                api,
                generation: 0,
                uid: None,
                profile: None,
                status: SessionStatus::Unknown,
                login_notice: None,
                changes,
            }),
            store,
            policy: Default::default(),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, SessionState>> {
        self.state.lock().map_err(|_| Error::LockPoisoned)
    }

    pub fn token(&self) -> Result<SessionToken> {
        Ok(self.lock()?.token())
    }

    pub fn subscribe(&self) -> Result<tokio::sync::watch::Receiver<SessionToken>> {
        Ok(self.lock()?.changes.subscribe())
    }

    /// Native lifecycle adapters can recover the concrete transport bound to
    /// this generation, rather than reopen a different credential directory.
    pub(crate) fn native_api(&self, token: &SessionToken) -> Result<Arc<dyn Api>> {
        let state = self.lock()?;
        state.require(token)?;
        Ok(state.api.clone())
    }

    pub(crate) fn persistent_store(&self) -> Result<Arc<StateStore>> {
        self.store
            .clone()
            .ok_or(Error::State("no persistent account store"))
    }

    pub(crate) fn with_token<T>(
        &self,
        token: &SessionToken,
        operation: impl FnOnce(&str) -> Result<T>,
    ) -> Result<T> {
        let state = self.lock()?;
        state.require(token)?;
        operation(
            state
                .uid
                .as_deref()
                .ok_or(Error::State("no authenticated account"))?,
        )
    }

    /// Local playback and lyric preferences follow account generations while
    /// retaining a separate guest namespace. This never authorizes online APIs.
    pub(crate) fn with_local_owner<T>(
        &self,
        token: &SessionToken,
        operation: impl FnOnce(&str) -> Result<T>,
    ) -> Result<T> {
        let state = self.lock()?;
        state.require(token)?;
        operation(state.uid.as_deref().unwrap_or("local-guest"))
    }
    pub(crate) fn check_generation(&self, token: &SessionToken) -> Result<()> {
        self.lock()?.require(token)
    }

    pub async fn run_for<T, F, Fut>(&self, expected: &SessionToken, operation: F) -> Result<T>
    where
        F: FnOnce(Arc<dyn Api>, String) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let (token, value) = self
            .run(|api, uid| {
                let checked = self.lock().and_then(|state| state.require(expected));
                async move {
                    checked?;
                    operation(api, uid).await
                }
            })
            .await?;
        if token != *expected {
            return Err(Error::StaleResponse);
        }
        Ok(value)
    }

    pub(crate) fn require_api(&self, expected: &SessionToken, api: &Arc<dyn Api>) -> Result<()> {
        let state = self.lock()?;
        state.require(expected)?;
        if !Arc::ptr_eq(&state.api, api) {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    pub(crate) fn commit_refreshed_profile(
        &self,
        expected: &SessionToken,
        api: &Arc<dyn Api>,
        profile: Value,
    ) -> Result<()> {
        let uid = identifier(&profile["userId"])?;
        let mut state = self.lock()?;
        state.require(expected)?;
        if !Arc::ptr_eq(&state.api, api) {
            return Err(Error::StaleResponse);
        }
        if expected.uid.as_deref() != Some(&uid) {
            state.invalidate(SessionStatus::Expired)?;
            return Err(Error::Response("refresh changed account identity"));
        }
        if let Some(store) = &self.store {
            store.put(&uid, "account", "profile", &profile)?;
        }
        state.profile = Some(profile);
        Ok(())
    }

    pub(crate) fn clear_current(
        &self,
        expected: &SessionToken,
        api: &Arc<dyn Api>,
        clear: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let mut state = self.lock()?;
        state.require(expected)?;
        if !Arc::ptr_eq(&state.api, api) {
            return Err(Error::StaleResponse);
        }
        let generation = next_generation(state.generation)?;
        let native: Arc<dyn std::any::Any + Send + Sync> = api.clone();
        if let Ok(client) = native.downcast::<crate::protocol::http::MusicHttpClient>() {
            self.recover_queue_reset(&client)?;
        }
        clear()?;
        state.generation = generation;
        state.uid = None;
        state.profile = None;
        state.status = SessionStatus::SignedOut;
        state.login_notice = None;
        state.changes.send_replace(state.token());
        Ok(())
    }

    pub fn snapshot(&self) -> Result<SessionSnapshot> {
        let state = self.lock()?;
        Ok(SessionSnapshot {
            token: state.token(),
            status: state.status,
            profile: state.profile.clone(),
            login_notice: state.login_notice.clone(),
        })
    }

    pub fn attach(&self, api: Arc<dyn Api>) -> Result<()> {
        let mut state = self.lock()?;
        state.invalidate(SessionStatus::Unknown)?;
        state.api = api;
        Ok(())
    }

    /// Commit an independently authenticated candidate, after account/get.
    pub fn adopt(
        &self,
        api: Arc<dyn Api>,
        profile: Value,
        expected: &SessionToken,
    ) -> Result<Value> {
        self.adopt_inner(api, profile, expected, None, |_| Ok(()))
    }

    pub(crate) fn adopt_registered(
        &self,
        api: Arc<dyn Api>,
        profile: Value,
        expected: &SessionToken,
        registry: &Arc<StateStore>,
        select: impl FnOnce(&mut StateTransaction<'_>) -> Result<()>,
    ) -> Result<Value> {
        self.adopt_inner(api, profile, expected, Some(registry), select)
    }

    fn adopt_inner(
        &self,
        api: Arc<dyn Api>,
        profile: Value,
        expected: &SessionToken,
        registry: Option<&Arc<StateStore>>,
        select: impl FnOnce(&mut StateTransaction<'_>) -> Result<()>,
    ) -> Result<Value> {
        if let Some(registry) = registry
            && !self
                .store
                .as_ref()
                .is_some_and(|store| Arc::ptr_eq(store, registry))
        {
            return Err(Error::State(
                "account session must use the candidate registry store",
            ));
        }
        let uid = identifier(&profile["userId"])?;
        let mut state = self.lock()?;
        state.require(expected)?;
        let generation = next_generation(state.generation)?;
        if let Some(store) = &self.store {
            store.transaction(|tx| {
                tx.put(&uid, "account", "profile", &profile)?;
                select(tx)
            })?;
        }
        state.generation = generation;
        state.api = api;
        state.uid = Some(uid);
        state.profile = Some(profile.clone());
        state.status = SessionStatus::Authenticated;
        state.login_notice = None;
        state.changes.send_replace(state.token());
        Ok(profile)
    }

    pub async fn refresh(&self) -> Result<Value> {
        let (token, api) = {
            let state = self.lock()?;
            (state.token(), state.api.clone())
        };
        let native: Arc<dyn std::any::Any + Send + Sync> = api.clone();
        if let Ok(client) = native.downcast::<crate::protocol::http::MusicHttpClient>()
            && let Some(notice) = client.credentials().invalid_login_notice()?
        {
            let mut state = self.lock()?;
            state.require(&token)?;
            state.login_notice = Some(notice);
            if state.status != SessionStatus::Expired {
                state.invalidate(SessionStatus::Expired)?;
            }
            self.recover_queue_reset(&client)?;
            return Err(Error::service("session", json!(301), Value::Null));
        }
        let profile = match api.account().await {
            Ok(profile) => profile,
            Err(error) => return Err(self.reject(&token, error).await?),
        };
        let uid = identifier(&profile["userId"])?;
        let mut state = self.lock()?;
        state.require(&token)?;
        let generation = if state.uid.as_ref() != Some(&uid) {
            next_generation(state.generation)?
        } else {
            state.generation
        };
        // Persist before publishing authenticated memory state. A disk failure
        // cannot report a successful login whose profile was not committed.
        if let Some(store) = &self.store {
            store.put(&uid, "account", "profile", &profile)?;
        }
        state.generation = generation;
        state.uid = Some(uid);
        state.profile = Some(profile.clone());
        state.status = SessionStatus::Authenticated;
        state.login_notice = None;
        if state.token() != token {
            state.changes.send_replace(state.token());
        }
        Ok(profile)
    }

    async fn reject(&self, token: &SessionToken, error: Error) -> Result<Error> {
        let intercepted = self.intercept_login_error(token, &error).await?;
        let mut state = self.lock()?;
        state.require(token)?;
        if let Some(outcome) = intercepted {
            if let policy::Interception::Invalid {
                notice,
                clear_queue,
            } = outcome
            {
                let native: Arc<dyn std::any::Any + Send + Sync> = state.api.clone();
                let client = native
                    .downcast::<crate::protocol::http::MusicHttpClient>()
                    .map_err(|_| Error::State("native login invalidation unavailable"))?;
                let cleared = self.invalidate_native_login(&client, token, &notice, clear_queue);
                state.login_notice = Some(notice);
                state.invalidate(SessionStatus::Expired)?;
                cleared?;
                // The PC factory normalizes string response codes before its
                // 301 event. Keep that meaning for downstream auth consumers.
                return Ok(if error.is_service_code(301) {
                    error
                } else {
                    Error::service("session", json!(301), Value::Null)
                });
            }
            return Ok(Error::LoginDeferred);
        }
        if error.is_service_code(301) {
            state.invalidate(SessionStatus::Expired)?;
        }
        Ok(error)
    }

    pub async fn run<T, F, Fut>(&self, operation: F) -> Result<(SessionToken, T)>
    where
        F: FnOnce(Arc<dyn Api>, String) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let (token, api, uid) = {
            let state = self.lock()?;
            if state.status != SessionStatus::Authenticated {
                return Err(Error::service("session", json!(301), Value::Null));
            }
            (
                state.token(),
                state.api.clone(),
                state.uid.clone().ok_or(Error::State("missing account"))?,
            )
        };
        let value = match operation(api, uid).await {
            Ok(value) => value,
            Err(error) => return Err(self.reject(&token, error).await?),
        };
        self.lock()?.require(&token)?;
        Ok((token, value))
    }

    pub fn save_result(
        &self,
        token: &SessionToken,
        kind: &str,
        key: &str,
        value: &Value,
    ) -> Result<()> {
        let state = self.lock()?;
        state.require(token)?;
        let uid = state
            .uid
            .as_deref()
            .ok_or(Error::State("no authenticated account"))?;
        let store = self
            .store
            .as_ref()
            .ok_or(Error::State("no persistent store"))?;
        store.put(uid, kind, key, value)
    }

    /// Local invalidation only. Server logout and credential removal are
    /// separate use cases, and are not implied by this method.
    pub fn detach(&self) -> Result<()> {
        self.lock()?.invalidate(SessionStatus::SignedOut)
    }
}
