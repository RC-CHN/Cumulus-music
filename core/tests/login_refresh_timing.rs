mod support;
use cumulus_core::{
    Error,
    account::{
        AccountSession,
        credentials::Credentials,
        login::{CredentialLogin, LoginProtocol, LoginStatus, ResendCooldown},
        profiles::ProfileRegistry,
        refresh::{AccountLifecycle, DeviceInfo, DeviceTokenProvider, RefreshOutcome},
    },
    protocol::{api::ApiFuture, http::ApiOrigin},
    storage::StateStore,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use support::{
    ScriptApi,
    http::{HttpResponse, ScriptHttp, reply},
};

struct Provider(AtomicU64);
impl DeviceTokenProvider for Provider {
    fn token(&self) -> ApiFuture<'_, String> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(String::new())
        })
    }
}
fn authorized() -> HttpResponse {
    let mut response = reply(json!({"code":200}));
    response.set_cookies = vec!["MUSIC_U=synthetic-login; Path=/; Secure".into()];
    response.refresh_token = Some("synthetic-refresh".into());
    HttpResponse::Ready(Ok(response))
}
fn identity() -> HttpResponse {
    json!({"code":200,"profile":{"userId":1}}).into()
}
struct World {
    _root: tempfile::TempDir,
    registry: Arc<ProfileRegistry>,
    session: Arc<AccountSession>,
    credentials: Arc<Credentials>,
    http: Arc<ScriptHttp>,
    clock: Arc<AtomicU64>,
    lifecycle: AccountLifecycle,
    provider: Provider,
    device: DeviceInfo,
}
impl World {
    async fn login(
        protocol: LoginProtocol,
        replies: impl IntoIterator<Item = HttpResponse>,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let session = Arc::new(AccountSession::new(
            Arc::new(ScriptApi::new([])),
            Some(registry.store()),
        ));
        let http = Arc::new(ScriptHttp::new(
            [authorized(), identity()].into_iter().chain(replies),
        ));
        let candidate = registry
            .candidate_with_backend(Default::default(), Default::default(), None, http.clone())
            .unwrap();
        let credentials = candidate.credentials();
        let client = candidate.client();
        let flow = CredentialLogin::new_with_protocol(
            session.clone(),
            candidate,
            Arc::new(ResendCooldown::default()),
            None,
            protocol,
        )
        .unwrap();
        flow.email("me@example.test", "synthetic-password", true, None)
            .await
            .unwrap();
        assert_eq!(flow.snapshot().unwrap().status, LoginStatus::Authenticated);
        let clock = Arc::new(AtomicU64::new(
            credentials
                .login_credentials()
                .unwrap()
                .last_login_at
                .unwrap_or(100000),
        ));
        let lifecycle = AccountLifecycle::with_clock(session.clone(), client.clone(), {
            let clock = clock.clone();
            Arc::new(move || Ok(clock.load(Ordering::SeqCst)))
        });
        let device = DeviceInfo {
            device_id: credentials.device_id().unwrap(),
            ..Default::default()
        };
        Self {
            _root: root,
            registry,
            session,
            credentials,
            http,
            clock,
            lifecycle,
            provider: Provider(AtomicU64::new(0)),
            device,
        }
    }
    async fn check(&self) -> RefreshOutcome {
        self.lifecycle
            .refresh_jsk(&self.device, &self.provider, false)
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn new_jsk_login_waits_a_full_day_without_claiming_renewal_or_calling_provider() {
    let w = World::login(LoginProtocol::Jsk, [authorized(), identity()]).await;
    let at = w
        .credentials
        .login_credentials()
        .unwrap()
        .last_login_at
        .expect("JSK receipt must persist its login time");
    assert!(at > 0);
    for now in [at, at - 1, at + 86399] {
        w.clock.store(now, Ordering::SeqCst);
        assert_eq!(w.check().await, RefreshOutcome::NotDue);
        assert_eq!(w.provider.0.load(Ordering::SeqCst), 0);
        let status = w.lifecycle.refresh_status().unwrap();
        assert!(!status.pending && status.last_success.is_none());
        assert_eq!(w.http.calls().len(), 2);
    }
    w.clock.store(at + 86400, Ordering::SeqCst);
    assert_eq!(w.check().await, RefreshOutcome::Refreshed);
    assert_eq!(w.provider.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        w.credentials.login_credentials().unwrap().last_login_at,
        Some(at)
    );
    assert_eq!(
        w.lifecycle.refresh_status().unwrap().last_success,
        Some(at + 86400)
    );
    w.clock.store(at + 86401, Ordering::SeqCst);
    assert_eq!(w.check().await, RefreshOutcome::NotDue);
    assert_eq!(w.http.calls().len(), 4);
}

#[tokio::test]
async fn saved_account_identity_reads_cookie_updates_and_restart_preserve_login_baseline() {
    let w = World::login(LoginProtocol::Jsk, []).await;
    let at = w.credentials.login_credentials().unwrap().last_login_at;
    assert!(at.is_some());
    let target = ApiOrigin::Pc.url();
    let before = w.credentials.request_credentials(&target).unwrap();
    w.credentials
        .record_response(
            &target,
            &["ordinary=read-update; Path=/; Secure".into()],
            None,
            &before.revision,
        )
        .unwrap();
    let reopened = w
        .registry
        .current(Default::default())
        .unwrap()
        .unwrap()
        .credentials;
    assert_eq!(reopened.login_credentials().unwrap().last_login_at, at);
    let http = Arc::new(ScriptHttp::new([identity()]));
    let selected_id = w.registry.selection().unwrap().unwrap().session_id;
    let saved = w
        .registry
        .saved_candidate_with_backend(
            &selected_id,
            Default::default(),
            Default::default(),
            None,
            http.clone(),
        )
        .unwrap();
    saved
        .verify_and_adopt(&w.session, &w.session.token().unwrap())
        .await
        .unwrap();
    let selected = w.registry.current(Default::default()).unwrap().unwrap();
    let lifecycle = AccountLifecycle::with_clock(w.session.clone(), saved.client(), {
        let at = at.unwrap();
        Arc::new(move || Ok(at + 86399))
    });
    assert_eq!(
        selected
            .credentials
            .login_credentials()
            .unwrap()
            .last_login_at,
        at
    );
    let device = DeviceInfo {
        device_id: selected.credentials.device_id().unwrap(),
        ..Default::default()
    };
    assert_eq!(
        lifecycle
            .refresh_jsk(&device, &w.provider, false)
            .await
            .unwrap(),
        RefreshOutcome::NotDue
    );
    assert!(lifecycle.refresh_status().unwrap().last_success.is_none());
    assert_eq!(http.calls().len(), 1);
    assert_eq!(w.provider.0.load(Ordering::SeqCst), 0);
    selected.credentials.clear().unwrap();
    assert!(
        selected
            .credentials
            .login_credentials()
            .unwrap()
            .last_login_at
            .is_none()
    );
}

#[tokio::test]
async fn unknown_rotation_takes_precedence_over_a_recent_login_and_is_never_replayed() {
    let w = World::login(
        LoginProtocol::Jsk,
        [HttpResponse::Ready(Err(Error::Transport(
            "synthetic lost response".into(),
        )))],
    )
    .await;
    assert!(
        w.lifecycle
            .refresh_jsk(&w.device, &w.provider, true)
            .await
            .is_err()
    );
    assert_eq!(w.check().await, RefreshOutcome::RecoveryRequired);
    assert_eq!(
        w.lifecycle.reconcile_jsk().await.unwrap(),
        RefreshOutcome::ReauthenticationRequired
    );
    assert_eq!(w.http.calls().len(), 3);
    assert_eq!(w.provider.0.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn legacy_login_and_older_records_do_not_invent_a_recent_jsk_login_time() {
    let w = World::login(LoginProtocol::Legacy, [authorized(), identity()]).await;
    assert!(
        w.credentials
            .login_credentials()
            .unwrap()
            .last_login_at
            .is_none()
    );
    assert_eq!(w.check().await, RefreshOutcome::Refreshed);
    let selected = w.registry.selection().unwrap().unwrap();
    let path = w._root.path().join("sessions").join(selected.session_id);
    let store = Arc::new(StateStore::open(&path).unwrap());
    let mut raw = store
        .get("transport", "credentials", "session")
        .unwrap()
        .unwrap();
    raw.as_object_mut().unwrap().remove("last_login_at");
    store
        .put("transport", "credentials", "session", &raw)
        .unwrap();
    let old = Credentials::open(store, ApiOrigin::Pc).unwrap();
    assert!(old.login_credentials().unwrap().last_login_at.is_none());
    // The new field stays native-only, as do all credential receipts.
    assert!(
        !serde_json::to_string(&w.lifecycle.refresh_status().unwrap())
            .unwrap()
            .contains("last_login_at")
    );
}
