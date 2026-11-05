//! PC M/198's scheduled legacy Cookie renewal, separate from JSK and 301 validation.
use crate::{
    Error, Result,
    account::{
        AccountSession, SessionToken, credentials::RequestCredentials, identity::VerifiedAccount,
    },
    protocol::{
        api::Api,
        http::{ApiOrigin, MusicHttpClient, WireRoute},
    },
    storage::StateStore,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{sync::watch, task::JoinHandle, time::Instant};
use url::Url;

pub(crate) const LEGACY_PERIODIC_PATH: &str = "/api/login/token/refresh";
pub(super) const KIND: &str = "login_refresh";
pub(super) const KEY: &str = "legacy-periodic";
const PERIOD: Duration = Duration::from_secs(20 * 60 * 60);
const RETRY: Duration = Duration::from_secs(60 * 60);
const DELAY: Duration = Duration::from_secs(3);
const DEADLINE: Duration = Duration::from_secs(45);
const IDENTITY_PATH: &str = "/api/w/nuser/account/get";

#[derive(Default, Deserialize, Serialize)]
struct Record {
    pending: Option<String>,
    last_success: Option<u64>,
}

fn record(value: Option<Value>) -> Result<Record> {
    Ok(value
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default())
}

pub(super) fn pending(value: Option<Value>) -> Result<bool> {
    Ok(record(value)?.pending.is_some())
}

struct Intent<'a> {
    store: &'a StateStore,
    uid: &'a str,
    id: String,
    dispatched: AtomicBool,
}

impl<'a> Intent<'a> {
    fn begin(store: &'a StateStore, uid: &'a str) -> Result<Option<Self>> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let acquired = store.transaction(|tx| {
            let mut value = record(tx.get(uid, KIND, KEY)?)?;
            if value.pending.is_some() || super::journal::pending(tx.get(uid, KIND, "jsk")?)? {
                return Ok(false);
            }
            value.pending = Some(id.clone());
            tx.put(uid, KIND, KEY, &serde_json::to_value(value)?)?;
            Ok(true)
        })?;
        Ok(acquired.then(|| Self {
            store,
            uid,
            id,
            dispatched: AtomicBool::new(false),
        }))
    }

    fn finish(&self, success: Option<u64>) -> Result<()> {
        self.store.transaction(|tx| {
            let mut current = record(tx.get(self.uid, KIND, KEY)?)?;
            if current.pending.as_deref() != Some(self.id.as_str()) {
                return Err(Error::StaleResponse);
            }
            current.pending = None;
            if let Some(at) = success {
                current.last_success = Some(at);
            }
            tx.put(self.uid, KIND, KEY, &serde_json::to_value(current)?)
        })
    }
}

impl Drop for Intent<'_> {
    fn drop(&mut self) {
        if !self.dispatched.load(Ordering::Relaxed) {
            let _ = self.finish(None);
        }
    }
}

enum Step {
    Success,
    Retry,
    Suspend,
}

struct AccountTransport {
    api: Arc<dyn Api>,
    client: Arc<MusicHttpClient>,
    target: Url,
    identity_target: Url,
    before: RequestCredentials,
}

fn account_transport(
    session: &AccountSession,
    token: &SessionToken,
) -> Result<Option<AccountTransport>> {
    let Some(uid) = token.uid.as_deref() else {
        return Ok(None);
    };
    let api = session.native_api(token)?;
    let native: Arc<dyn std::any::Any + Send + Sync> = api.clone();
    let Ok(client) = native.downcast::<MusicHttpClient>() else {
        return Ok(None);
    };
    let credentials = client.credentials();
    let store = credentials.store();
    if credentials.origin() != ApiOrigin::Pc
        || pending(store.get(uid, KIND, KEY)?)?
        || super::journal::load(&store, uid)?.is_some_and(|value| value.pending)
    {
        return Ok(None);
    }
    let target = ApiOrigin::Pc.request_url(WireRoute::Eapi, LEGACY_PERIODIC_PATH)?;
    let identity_target = ApiOrigin::Pc.request_url(WireRoute::Eapi, IDENTITY_PATH)?;
    let before = credentials.request_credentials(&target)?;
    if before.unique_music_u().is_none()
        || credentials
            .request_credentials(&identity_target)?
            .unique_music_u()
            != before.unique_music_u()
    {
        return Ok(None);
    }
    Ok(Some(AccountTransport {
        api,
        client,
        target,
        identity_target,
        before,
    }))
}

async fn attempt(session: &AccountSession, token: &SessionToken) -> Result<Step> {
    let Some(AccountTransport {
        api,
        client,
        target,
        identity_target,
        before,
    }) = account_transport(session, token)?
    else {
        return Ok(Step::Suspend);
    };
    let uid = token.uid.as_deref().ok_or(Error::StaleResponse)?;
    match session.legacy_refresh_assignment(token).await? {
        Some(true) => (),
        Some(false) => return Ok(Step::Suspend),
        None => return Ok(Step::Retry),
    }
    session.require_api(token, &api)?;
    let credentials = client.credentials();
    let store = credentials.store();
    let Some(intent) = Intent::begin(&store, uid)? else {
        return Ok(Step::Suspend);
    };
    let allowed = || {
        session.require_api(token, &api)?;
        if session.observed_legacy_refresh_assignment(token)? != Some(true) {
            return Err(Error::Cancelled);
        }
        Ok(())
    };
    let response = client
        .request_legacy_periodic_refresh(&before.revision, &allowed, &intent.dispatched)
        .await;
    session.require_api(token, &api)?;
    let response = match response {
        Ok(value) => value,
        Err(_) if !intent.dispatched.load(Ordering::Relaxed) => return Ok(Step::Retry),
        Err(_) => return Ok(Step::Suspend),
    };
    if !crate::error::code_matches(&response["code"], 200) {
        let code = response["code"]
            .as_i64()
            .or_else(|| response["code"].as_str()?.parse().ok());
        return if code.is_some_and(|value| value != 0) {
            intent.finish(None)?;
            Ok(Step::Retry)
        } else {
            Ok(Step::Suspend)
        };
    }

    // Acknowledged legacy success does not require new credential headers.
    // Verify usable same-UID identity before recording only legacy timing.
    let after = credentials.request_credentials(&target)?;
    let effective = after.unique_music_u();
    if effective.is_none()
        || credentials
            .request_credentials(&identity_target)?
            .unique_music_u()
            != effective
    {
        return Ok(Step::Suspend);
    }
    let identity = VerifiedAccount::read(client.as_ref()).await?;
    if credentials.request_credentials(&target)?.unique_music_u() != effective
        || credentials
            .request_credentials(&identity_target)?
            .unique_music_u()
            != effective
    {
        return Ok(Step::Suspend);
    }
    session.commit_refreshed_profile(token, &api, identity)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::State("system clock precedes UNIX epoch"))?
        .as_secs();
    session.with_token(token, |_| intent.finish(Some(now)))?;
    Ok(Step::Success)
}

async fn run_generation(session: &AccountSession, token: &SessionToken) -> Result<()> {
    let start = Instant::now();
    if account_transport(session, token)?.is_none() {
        return Ok(());
    }
    if matches!(
        tokio::time::timeout(DEADLINE, session.legacy_refresh_assignment(token)).await,
        Ok(Ok(Some(false)))
    ) {
        return Ok(());
    }
    let mut interval = start + PERIOD + DELAY;
    let mut retry = session
        .legacy_refresh_start_immediately(token)?
        .then_some(start + DELAY);
    loop {
        let due = retry.map_or(interval, |retry| retry.min(interval));
        tokio::time::sleep_until(due).await;
        if due == interval {
            // After a long suspend skip missed ticks rather than bursting.
            interval += PERIOD;
            if interval <= Instant::now() {
                interval = Instant::now() + PERIOD;
            }
        }
        retry = match tokio::time::timeout(DEADLINE, attempt(session, token)).await {
            Ok(Ok(Step::Success)) => None,
            Ok(Ok(Step::Retry)) => Some(Instant::now() + RETRY + DELAY),
            Ok(Ok(Step::Suspend) | Err(_)) | Err(_) => return Ok(()),
        };
    }
}

async fn run(session: Arc<AccountSession>, mut changes: watch::Receiver<SessionToken>) {
    loop {
        let token = changes.borrow_and_update().clone();
        if token.uid.is_some() {
            tokio::select! {
                biased;
                changed = changes.changed() => {
                    if changed.is_err() { break; }
                    continue;
                },
                _ = run_generation(&session, &token) => (),
            }
        }
        if changes.changed().await.is_err() {
            break;
        }
    }
}

struct Lease(Arc<AccountSession>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0
            .legacy_renewal_running
            .store(false, Ordering::Release);
    }
}

/// One native worker for this session; drop it when the host shuts down.
pub struct LegacyRenewal(JoinHandle<()>);

impl LegacyRenewal {
    pub(crate) fn start(session: Arc<AccountSession>) -> Result<Self> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| Error::State("legacy renewal requires the host runtime"))?;
        let changes = session.subscribe()?;
        if session.legacy_renewal_running.swap(true, Ordering::AcqRel) {
            return Err(Error::MutationBusy);
        }
        // A lease captured by the future also releases before its first poll.
        let lease = Lease(session.clone());
        Ok(Self(runtime.spawn(async move {
            let _lease = lease;
            run(session, changes).await;
        })))
    }
}

impl Drop for LegacyRenewal {
    fn drop(&mut self) {
        self.0.abort();
    }
}
