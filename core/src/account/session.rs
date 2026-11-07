//! Account-generation and QR state machines recovered from the official host.
//! Network awaits never hold the state lock. QR authorization does not itself
//! authenticate an account; identity must still be read and committed.
mod invalidation;
mod legacy_api;
mod policy;
mod remove_guide;
use super::{artist_onboarding::ArtistLogin, identity::VerifiedAccount, login_policy::LoginNotice};
use crate::{
    Error, Result,
    membership::MembershipCommit,
    protocol::api::{Api, identifier},
    storage::{StateStore, StateTransaction},
};
use remove_guide::ClassicRemoveGuide;
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
    pub(crate) classic_remove_guide: Option<String>,
}

/// Only the current account's verified entitlement changes cross this boundary.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipObservation {
    pub epoch: String,
    pub revision: String,
}

struct ObservedMembership {
    token: SessionToken,
    tuple: ([bool; 5], String),
    server_now: f64,
    observation: MembershipObservation,
}

struct SessionState {
    api: Arc<dyn Api>,
    generation: u64,
    uid: Option<String>,
    profile: Option<Value>,
    status: SessionStatus,
    guest_ready: bool,
    login_notice: Option<LoginNotice>,
    classic_remove_guide: Option<ClassicRemoveGuide>,
    initial_restore_generation: Option<u64>,
    quick_exchange_generation: Option<u64>,
    artist_login: Option<ArtistLogin>,
    registration_type: Option<u32>,
    changes: tokio::sync::watch::Sender<SessionToken>,
    membership: Option<ObservedMembership>,
    membership_changes: tokio::sync::watch::Sender<Option<MembershipObservation>>,
    membership_commits: tokio::sync::watch::Sender<Option<MembershipCommit>>,
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
        self.classic_remove_guide = None;
        self.uid = None;
        self.profile = None;
        self.artist_login = None;
        self.registration_type = None;
        self.status = status;
        self.guest_ready = false;
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
    pub(super) legacy_renewal_running: std::sync::atomic::AtomicBool,
}

#[derive(Default)]
pub(super) struct Adoption {
    pub artist_login: Option<ArtistLogin>,
    pub quick_exchange: bool,
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
                guest_ready: false,
                login_notice: None,
                classic_remove_guide: None,
                initial_restore_generation: None,
                quick_exchange_generation: None,
                artist_login: None,
                registration_type: None,
                changes,
                membership: None,
                membership_changes: tokio::sync::watch::channel(None).0,
                membership_commits: tokio::sync::watch::channel(None).0,
            }),
            store,
            policy: Default::default(),
            legacy_renewal_running: Default::default(),
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

    pub(crate) fn subscribe_membership(
        &self,
    ) -> Result<tokio::sync::watch::Receiver<Option<MembershipObservation>>> {
        Ok(self.lock()?.membership_changes.subscribe())
    }

    pub(crate) fn membership_observation(&self, token: &SessionToken) -> Result<Option<String>> {
        let state = self.lock()?;
        state.require(token)?;
        Ok(state
            .membership
            .as_ref()
            .filter(|entry| entry.token == *token)
            .map(|entry| entry.observation.revision.clone()))
    }

    pub(crate) fn subscribe_membership_commits(
        &self,
    ) -> Result<tokio::sync::watch::Receiver<Option<MembershipCommit>>> {
        Ok(self.lock()?.membership_commits.subscribe())
    }

    pub(crate) fn commit_membership<T>(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: Option<&str>,
        commit: MembershipCommit,
        tuple: ([bool; 5], String),
        persist: impl FnOnce(&str) -> Result<T>,
    ) -> Result<T> {
        let mut state = self.lock()?;
        state.require(token)?;
        if !Arc::ptr_eq(&state.api, api)
            || api.membership_credential_revision()?.as_deref() != credentials
        {
            return Err(Error::StaleResponse);
        }
        if state
            .membership
            .as_ref()
            .is_some_and(|old| old.token == *token && commit.server_now < old.server_now)
        {
            return Err(Error::StaleResponse);
        }
        let uid = state.uid.as_deref().ok_or(Error::StaleResponse)?;
        let value = persist(uid)?;
        // Publishing the deadline is separate from the UI rights tuple. A
        // shifted expiry can require rearming without changing any active flag.
        if let Some(old) = state
            .membership
            .as_mut()
            .filter(|entry| entry.token == *token)
            && tuple == old.tuple
        {
            old.server_now = commit.server_now;
            state.membership_commits.send_replace(Some(commit));
            return Ok(value);
        }
        let observation = MembershipObservation {
            epoch: token.generation.to_string(),
            revision: uuid::Uuid::new_v4().simple().to_string(),
        };
        state.membership = Some(ObservedMembership {
            token: token.clone(),
            tuple,
            server_now: commit.server_now,
            observation: observation.clone(),
        });
        state.membership_commits.send_replace(Some(commit));
        state.membership_changes.send_replace(Some(observation));
        Ok(value)
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

    /// A synchronous PC presentation commit must not cross a native API or
    /// account handoff. The callback must not re-enter AccountSession.
    pub(crate) fn with_pc_presentation_identity<T>(
        &self,
        expected: &SessionToken,
        api: &Arc<dyn Api>,
        operation: impl FnOnce(Option<&str>, bool, bool) -> Result<T>,
    ) -> Result<T> {
        let state = self.lock()?;
        state.require(expected)?;
        if !Arc::ptr_eq(&state.api, api) {
            return Err(Error::StaleResponse);
        }
        let verified_uid = state
            .uid
            .as_deref()
            .filter(|_| state.status == SessionStatus::Authenticated && state.profile.is_some());
        let no_uid = state.uid.is_none();
        let guest_ready = no_uid && (state.guest_ready || state.status == SessionStatus::SignedOut);
        operation(verified_uid, no_uid, guest_ready)
    }

    /// A signed-out guest may move to an isolated native anonymous jar. This
    /// never adopts its anonymous UID as an authenticated account, nor changes
    /// the guest epoch. API and presentation observers must still rebind.
    pub(crate) fn replace_guest_api(
        &self,
        expected: &SessionToken,
        previous: &Arc<dyn Api>,
        next: Arc<dyn Api>,
    ) -> Result<()> {
        let mut state = self.lock()?;
        state.require(expected)?;
        if state.uid.is_some()
            || !(state.guest_ready || state.status == SessionStatus::SignedOut)
            || !Arc::ptr_eq(&state.api, previous)
        {
            return Err(Error::StaleResponse);
        }
        state.api = next;
        state.guest_ready = true;
        state.changes.send_replace(state.token());
        Ok(())
    }

    /// A fresh native 301 completes restoration without expiring a login or
    /// advancing the generation. Unknown before that read cannot register.
    pub(crate) fn guest_ready_for(
        &self,
        expected: &SessionToken,
        api: &Arc<dyn Api>,
    ) -> Result<bool> {
        let state = self.lock()?;
        state.require(expected)?;
        if !Arc::ptr_eq(&state.api, api) {
            return Err(Error::StaleResponse);
        }
        Ok(state.uid.is_none() && (state.guest_ready || state.status == SessionStatus::SignedOut))
    }

    pub(crate) fn with_verified_profile<T>(
        &self,
        expected: &SessionToken,
        api: &Arc<dyn Api>,
        operation: impl FnOnce(&str, &Value) -> Result<T>,
    ) -> Result<T> {
        let state = self.lock()?;
        state.require(expected)?;
        if !Arc::ptr_eq(&state.api, api) {
            return Err(Error::StaleResponse);
        }
        operation(
            state
                .uid
                .as_deref()
                .ok_or(Error::State("no authenticated account"))?,
            state
                .profile
                .as_ref()
                .ok_or(Error::State("no verified account profile"))?,
        )
    }

    pub(crate) fn commit_refreshed_profile(
        &self,
        expected: &SessionToken,
        api: &Arc<dyn Api>,
        identity: VerifiedAccount,
    ) -> Result<()> {
        let VerifiedAccount {
            profile,
            registration_type,
        } = identity;
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
        state.registration_type = registration_type;
        Ok(())
    }

    /// Publish a verified same-UID profile projection with its journal update.
    /// Absent detail fields leave the existing account/get metadata intact.
    pub(crate) fn commit_profile_edit(
        &self,
        expected: &SessionToken,
        api: &Arc<dyn Api>,
        verified: &super::profile::SelfProfileView,
        record: impl FnOnce(&mut StateTransaction<'_>, &str) -> Result<()>,
    ) -> Result<()> {
        let mut state = self.lock()?;
        state.require(expected)?;
        if !Arc::ptr_eq(&state.api, api)
            || state.status != SessionStatus::Authenticated
            || state.uid.as_deref() != Some(verified.uid.as_str())
        {
            return Err(Error::StaleResponse);
        }
        let mut profile = state
            .profile
            .clone()
            .ok_or(Error::State("missing authenticated profile"))?;
        let fields = profile
            .as_object_mut()
            .ok_or(Error::State("invalid authenticated profile"))?;
        for (key, value) in [
            ("nickname", json!(verified.nickname)),
            ("avatarUrl", json!(verified.avatar_url)),
            ("signature", json!(verified.signature)),
            ("gender", json!(verified.gender)),
            ("birthday", json!(verified.birthday)),
        ] {
            if !value.is_null() {
                fields.insert(key.into(), value);
            }
        }
        if let (Some(province), Some(city)) = (verified.province, verified.city) {
            fields.insert("province".into(), json!(province));
            fields.insert("city".into(), json!(city));
        }
        self.persistent_store()?.transaction(|tx| {
            tx.put(&verified.uid, "account", "profile", &profile)?;
            record(tx, &verified.uid)
        })?;
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
        state.artist_login = None;
        state.registration_type = None;
        state.status = SessionStatus::SignedOut;
        state.guest_ready = false;
        state.login_notice = None;
        state.classic_remove_guide = None;
        state.changes.send_replace(state.token());
        Ok(())
    }

    pub(crate) fn registration_type(&self, token: &SessionToken) -> Result<Option<u32>> {
        let state = self.lock()?;
        state.require(token)?;
        Ok(state.registration_type)
    }

    pub(crate) fn artist_login(&self, token: &SessionToken) -> Result<Option<ArtistLogin>> {
        let state = self.lock()?;
        state.require(token)?;
        Ok(state.artist_login.or_else(|| {
            (token.uid.is_some() && state.initial_restore_generation == Some(token.generation))
                .then_some(ArtistLogin { scene: None })
        }))
    }

    pub fn snapshot(&self) -> Result<SessionSnapshot> {
        let state = self.lock()?;
        Ok(SessionSnapshot {
            token: state.token(),
            status: state.status,
            profile: state.profile.clone(),
            login_notice: state.login_notice.clone(),
            classic_remove_guide: state
                .classic_remove_guide
                .as_ref()
                .and_then(|guide| guide.offer(state.generation)),
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
        self.adopt_inner(
            api,
            VerifiedAccount::profile_only(profile),
            expected,
            None,
            Adoption::default(),
            |_| Ok(()),
        )
    }

    pub(super) fn adopt_registered(
        &self,
        api: Arc<dyn Api>,
        identity: VerifiedAccount,
        expected: &SessionToken,
        registry: &Arc<StateStore>,
        activation: Adoption,
        select: impl FnOnce(&mut StateTransaction<'_>) -> Result<()>,
    ) -> Result<Value> {
        self.adopt_inner(api, identity, expected, Some(registry), activation, select)
    }

    #[cfg(test)]
    pub(crate) fn adopt_for_artist_onboarding_test(
        &self,
        api: Arc<dyn Api>,
        identity: VerifiedAccount,
        registry: &Arc<StateStore>,
        artist_login: ArtistLogin,
    ) -> Result<Value> {
        self.adopt_registered(
            api,
            identity,
            &self.token()?,
            registry,
            Adoption {
                artist_login: Some(artist_login),
                quick_exchange: false,
            },
            |_| Ok(()),
        )
    }

    fn adopt_inner(
        &self,
        api: Arc<dyn Api>,
        identity: VerifiedAccount,
        expected: &SessionToken,
        registry: Option<&Arc<StateStore>>,
        activation: Adoption,
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
        let VerifiedAccount {
            profile,
            registration_type,
        } = identity;
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
        state.artist_login = activation.artist_login;
        state.quick_exchange_generation = activation.quick_exchange.then_some(generation);
        state.registration_type = registration_type;
        state.uid = Some(uid);
        state.profile = Some(profile.clone());
        state.status = SessionStatus::Authenticated;
        state.guest_ready = false;
        state.login_notice = None;
        state.classic_remove_guide = None;
        state.changes.send_replace(state.token());
        Ok(profile)
    }

    /// Only the first authoritative restore can carry a cold-start intent
    /// from the provisional local session. Interactive adoption cannot.
    pub(crate) fn is_initial_restore(
        &self,
        previous: &SessionToken,
        current: &SessionToken,
    ) -> Result<bool> {
        let state = self.lock()?;
        Ok(previous.generation == 0
            && previous.uid.is_none()
            && state.token() == *current
            && state.initial_restore_generation == Some(current.generation))
    }

    pub(crate) fn legacy_refresh_start_immediately(&self, token: &SessionToken) -> Result<bool> {
        let state = self.lock()?;
        state.require(token)?;
        Ok(state.status == SessionStatus::Authenticated
            && (state.initial_restore_generation == Some(token.generation)
                || state.quick_exchange_generation == Some(token.generation)))
    }

    pub async fn refresh(&self) -> Result<Value> {
        let (token, api) = {
            let state = self.lock()?;
            (state.token(), state.api.clone())
        };
        let native: Arc<dyn std::any::Any + Send + Sync> = api.clone();
        if native
            .downcast_ref::<crate::protocol::http::MusicHttpClient>()
            .is_some_and(crate::protocol::http::MusicHttpClient::is_isolated_anonymous)
        {
            // Anonymous adapter responses are never account-adoption input.
            // Existing explicit login flows still adopt their own candidate.
            self.require_api(&token, &api)?;
            return Err(Error::service("session", json!(301), Value::Null));
        }
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
        let VerifiedAccount {
            profile,
            registration_type,
        } = match VerifiedAccount::read(&*api).await {
            Ok(identity) => identity,
            Err(error)
                if error.is_service_code(301) && self.has_no_native_login(&token, &api)? =>
            {
                // account/get can reject a fresh installation or an explicitly
                // signed-out profile. There is no previous login to expire.
                let mut state = self.lock()?;
                state.require(&token)?;
                if !Arc::ptr_eq(&state.api, &api) {
                    return Err(Error::StaleResponse);
                }
                if !state.guest_ready {
                    state.guest_ready = true;
                    state.changes.send_replace(state.token());
                }
                return Err(error);
            }
            Err(error) => return Err(self.reject(&token, error).await?),
        };
        let uid = identifier(&profile["userId"])?;
        let mut state = self.lock()?;
        state.require(&token)?;
        if !Arc::ptr_eq(&state.api, &api) {
            return Err(Error::StaleResponse);
        }
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
        if token.generation == 0 && state.status == SessionStatus::Unknown && token.uid.is_none() {
            state.initial_restore_generation = Some(generation);
        }
        if generation != state.generation {
            state.artist_login = None;
        }
        state.generation = generation;
        state.uid = Some(uid);
        state.registration_type = registration_type;
        state.profile = Some(profile.clone());
        state.status = SessionStatus::Authenticated;
        state.guest_ready = false;
        state.login_notice = None;
        state.classic_remove_guide = None;
        if state.token() != token {
            state.changes.send_replace(state.token());
        }
        Ok(profile)
    }

    fn has_no_native_login(&self, token: &SessionToken, api: &Arc<dyn Api>) -> Result<bool> {
        let state = self.lock()?;
        state.require(token)?;
        if state.uid.is_some()
            || !matches!(
                state.status,
                SessionStatus::Unknown | SessionStatus::SignedOut
            )
        {
            return Ok(false);
        }
        let native: Arc<dyn std::any::Any + Send + Sync> = api.clone();
        let Ok(client) = native.downcast::<crate::protocol::http::MusicHttpClient>() else {
            return Ok(false);
        };
        let stored = client.credentials();
        let credentials = stored.login_credentials()?;
        Ok(credentials.token.is_none()
            && credentials.refresh_token.is_none()
            && stored.invalid_login_notice()?.is_none()
            && stored.pending_queue_reset()?.is_none())
    }

    async fn reject(&self, token: &SessionToken, error: Error) -> Result<Error> {
        let intercepted = self.intercept_login_error(token, &error).await?;
        let mut state = self.lock()?;
        state.require(token)?;
        if let Some(outcome) = intercepted {
            if let policy::Interception::Invalid {
                notice,
                clear_queue,
                classic_remove_guide,
            } = outcome
            {
                let native: Arc<dyn std::any::Any + Send + Sync> = state.api.clone();
                let client = native
                    .downcast::<crate::protocol::http::MusicHttpClient>()
                    .map_err(|_| Error::State("native login invalidation unavailable"))?;
                let show_classic_guide = classic_remove_guide
                    && state.status == SessionStatus::Authenticated
                    && notice.kind == super::login_policy::LoginNoticeKind::Reauthenticate;
                let cleared = self.invalidate_native_login(&client, token, &notice, clear_queue);
                state.login_notice = Some(notice);
                state.invalidate(SessionStatus::Expired)?;
                cleared?;
                if show_classic_guide {
                    state.classic_remove_guide = Some(ClassicRemoveGuide::new(state.generation));
                }
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
            let native: Arc<dyn std::any::Any + Send + Sync> = state.api.clone();
            if let (Some(store), Ok(client)) = (
                self.store.as_deref(),
                native.downcast::<crate::protocol::http::MusicHttpClient>(),
            ) {
                super::profiles::clear_binding_for(store, &client.credentials())?;
            }
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
