mod support;
use cumulus_core::{
    Error,
    account::{
        AccountSession, SessionStatus,
        credentials::{Credentials, LoginCredentials},
        refresh::{
            AccountLifecycle, DeviceInfo, DeviceTokenProvider, RefreshOutcome, encoded_model,
            encrypt_params, refresh_payload,
        },
    },
    protocol::{
        api::ApiFuture,
        http::{ApiOrigin, ClientProfile, MusicHttpClient, RequestOptions},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use support::http::{HttpResponse, ScriptHttp, gate, reply};

struct Tokens {
    calls: AtomicU64,
    value: &'static str,
}
impl Tokens {
    fn valid() -> Self {
        Self {
            calls: AtomicU64::new(0),
            value: "actual-test-provider",
        }
    }
}
impl DeviceTokenProvider for Tokens {
    fn token(&self) -> ApiFuture<'_, String> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.value.into())
        })
    }
}

struct World {
    directory: tempfile::TempDir,
    store: Arc<StateStore>,
    credentials: Arc<Credentials>,
    http: Arc<ScriptHttp>,
    client: Arc<MusicHttpClient>,
    session: Arc<AccountSession>,
    clock: Arc<AtomicU64>,
    manager: Arc<AccountLifecycle>,
    device: DeviceInfo,
}
impl World {
    async fn new(responses: impl IntoIterator<Item = HttpResponse>, has_refresh: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(directory.path()).unwrap());
        let credentials = Arc::new(Credentials::open(store.clone(), ApiOrigin::Pc).unwrap());
        let target = ApiOrigin::Pc.url();
        let before = credentials.request_credentials(&target).unwrap();
        credentials
            .record_response(
                &target,
                &["MUSIC_U=synthetic-old; Path=/; Secure".into()],
                has_refresh.then_some("synthetic-refresh-old"),
                &before.revision,
            )
            .unwrap();
        let mut replies = vec![json!({"code":200,"profile":{"userId":1}}).into()];
        replies.extend(responses);
        let http = Arc::new(ScriptHttp::new(replies));
        let client = Arc::new(
            MusicHttpClient::with_backend(
                credentials.clone(),
                ClientProfile::default(),
                RequestOptions::default(),
                None,
                http.clone(),
            )
            .unwrap(),
        );
        let session = Arc::new(AccountSession::new(client.clone(), Some(store.clone())));
        session.refresh().await.unwrap();
        let clock = Arc::new(AtomicU64::new(100000));
        let manager = Arc::new(AccountLifecycle::with_clock(
            session.clone(),
            client.clone(),
            {
                let clock = clock.clone();
                Arc::new(move || Ok(clock.load(Ordering::SeqCst)))
            },
        ));
        let device = DeviceInfo {
            device_name: "my,pc,2".into(),
            device_id: credentials.device_id().unwrap(),
            os: "linux".into(),
            ..Default::default()
        };
        Self {
            directory,
            store,
            credentials,
            http,
            client,
            session,
            clock,
            manager,
            device,
        }
    }
}
fn rotated() -> HttpResponse {
    let mut response = reply(json!({"code":200}));
    response.set_cookies = vec!["MUSIC_U=synthetic-new; Path=/; Secure".into()];
    response.refresh_token = Some("synthetic-refresh-new".into());
    HttpResponse::Ready(Ok(response))
}

#[test]
fn signature_and_model_match_fourteen_direct_official_function_results() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/login.json")).unwrap();
    for row in fixture["refresh"].as_array().unwrap() {
        assert_eq!(
            encrypt_params(&row["input"]["values"]).unwrap(),
            row["expected"]["sign"]
        );
        assert_eq!(
            encoded_model(row["input"]["model"].as_str()),
            row["expected"]["model"]
        );
    }
    let credentials = LoginCredentials {
        token: Some("U".into()),
        refresh_token: Some("R".into()),
        revision: "old".into(),
        rotation: None,
    };
    let device = DeviceInfo {
        device_name: "my,pc,2".into(),
        device_display_name: "fallback".into(),
        device_id: "D".into(),
        os: "os-fallback".into(),
        client_type: Some("linux".into()),
    };
    let payload = refresh_payload(&credentials, &json!(123), &device, "actual-provider").unwrap();
    assert_eq!(payload.as_object().unwrap().len(), 5);
    assert_eq!(payload["_sign"],encrypt_params(&json!({"refreshToken":"R","userId":"123","token":"U","model":"my.pc%2C2","os":"linux","deviceId":"D"})).unwrap());
    assert_eq!(
        refresh_payload(&credentials, &json!(123), &device, "").unwrap()["ydDeviceToken"],
        ""
    );
}

#[tokio::test]
async fn missing_refresh_credentials_stops_before_provider_or_network() {
    let world = World::new([], false).await;
    let tokens = Tokens::valid();
    assert_eq!(
        world
            .manager
            .refresh_jsk(&world.device, &tokens, false)
            .await
            .unwrap(),
        RefreshOutcome::MissingCredentials
    );
    assert_eq!(tokens.calls.load(Ordering::SeqCst), 0);
    assert_eq!(world.http.calls().len(), 1);
    assert!(
        world
            .store
            .get("1", "login_refresh", "jsk")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn acknowledged_pair_rotation_and_same_identity_commit_daily_clock_and_skip_backward_time() {
    let world = World::new(
        [
            rotated(),
            json!({"code":200,"profile":{"userId":1,"nickname":"same"}}).into(),
        ],
        true,
    )
    .await;
    let tokens = Tokens::valid();
    let before = world.credentials.login_credentials().unwrap();
    assert_eq!(
        world
            .manager
            .refresh_jsk(&world.device, &tokens, false)
            .await
            .unwrap(),
        RefreshOutcome::Refreshed
    );
    let after = world.credentials.login_credentials().unwrap();
    assert_ne!(before.rotation, after.rotation);
    assert_eq!(after.token.as_deref(), Some("synthetic-new"));
    assert_eq!(
        after.refresh_token.as_deref(),
        Some("synthetic-refresh-new")
    );
    assert_eq!(world.http.calls()[1].path, "/api/login/token/refresh/v1");
    assert_eq!(
        world.http.calls()[1].payload["ydDeviceToken"],
        "actual-test-provider"
    );
    assert_eq!(
        world.session.snapshot().unwrap().profile.unwrap()["nickname"],
        "same"
    );
    assert_eq!(
        world
            .manager
            .refresh_jsk(&world.device, &tokens, false)
            .await
            .unwrap(),
        RefreshOutcome::NotDue
    );
    world.clock.store(1, Ordering::SeqCst);
    assert_eq!(
        world
            .manager
            .refresh_jsk(&world.device, &tokens, false)
            .await
            .unwrap(),
        RefreshOutcome::NotDue
    );
    assert_eq!(world.http.calls().len(), 3);
    assert_eq!(tokens.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        world
            .store
            .get("1", "login_refresh", "jsk")
            .unwrap()
            .unwrap()["pending"],
        false
    );
}

#[tokio::test]
async fn incomplete_response_headers_and_unrelated_cookie_changes_cannot_claim_rotation() {
    for mode in 0..3 {
        let mut response = reply(json!({"code":200}));
        if mode == 0 {
            response.set_cookies = vec!["MUSIC_U=changed-alone; Path=/; Secure".into()];
        } else {
            response.refresh_token = Some("changed-refresh".into());
            response.set_cookies = vec![if mode == 1 {
                "unrelated=changed; Path=/; Secure".into()
            } else {
                "MUSIC_U=wrong-path; Path=/elsewhere; Secure".into()
            }];
        }
        let world = World::new([HttpResponse::Ready(Ok(response))], true).await;
        let tokens = Tokens::valid();
        let before = world.credentials.login_credentials().unwrap();
        assert_eq!(
            world
                .manager
                .refresh_jsk(&world.device, &tokens, true)
                .await
                .unwrap(),
            RefreshOutcome::ReauthenticationRequired
        );
        assert_eq!(
            world.credentials.login_credentials().unwrap().rotation,
            before.rotation
        );
        assert_eq!(
            world
                .manager
                .refresh_jsk(&world.device, &tokens, true)
                .await
                .unwrap(),
            RefreshOutcome::RecoveryRequired
        );
        assert_eq!(
            world.manager.reconcile_jsk().await.unwrap(),
            RefreshOutcome::ReauthenticationRequired
        );
        assert_eq!(world.http.calls().len(), 2);
        assert_eq!(tokens.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn lost_identity_reply_recovers_after_reopen_without_repeating_refresh() {
    let world = World::new(
        [
            rotated(),
            HttpResponse::Ready(Err(Error::Transport("synthetic identity timeout".into()))),
        ],
        true,
    )
    .await;
    assert!(
        world
            .manager
            .refresh_jsk(&world.device, &Tokens::valid(), false)
            .await
            .is_err()
    );
    let store = Arc::new(StateStore::open(world.directory.path()).unwrap());
    let credentials = Arc::new(Credentials::open(store.clone(), ApiOrigin::Pc).unwrap());
    let http = Arc::new(ScriptHttp::new([
        json!({"code":200,"profile":{"userId":1}}).into(),
        json!({"code":200,"profile":{"userId":1}}).into(),
    ]));
    let client = Arc::new(
        MusicHttpClient::with_backend(
            credentials,
            ClientProfile::default(),
            RequestOptions::default(),
            None,
            http.clone(),
        )
        .unwrap(),
    );
    let session = Arc::new(AccountSession::new(client.clone(), Some(store.clone())));
    session.refresh().await.unwrap();
    let manager = AccountLifecycle::new(session, client);
    assert_eq!(
        manager.reconcile_jsk().await.unwrap(),
        RefreshOutcome::Refreshed
    );
    assert!(
        http.calls()
            .iter()
            .all(|c| c.path == "/api/w/nuser/account/get")
    );
    assert_eq!(
        store.get("1", "login_refresh", "jsk").unwrap().unwrap()["pending"],
        false
    );
    assert_eq!(
        manager.reconcile_jsk().await.unwrap(),
        RefreshOutcome::NoPendingRefresh
    );
}

#[tokio::test]
async fn concurrent_managers_observe_durable_pending_intent_before_a_second_rotation() {
    let (pending, release) = gate();
    let world = World::new(
        [pending, json!({"code":200,"profile":{"userId":1}}).into()],
        true,
    )
    .await;
    let work = {
        let manager = world.manager.clone();
        let device = world.device.clone();
        tokio::spawn(async move { manager.refresh_jsk(&device, &Tokens::valid(), true).await })
    };
    world.http.wait(2).await;
    let other = AccountLifecycle::new(world.session.clone(), world.client.clone());
    let tokens = Tokens::valid();
    assert_eq!(
        other
            .refresh_jsk(&world.device, &tokens, true)
            .await
            .unwrap(),
        RefreshOutcome::RecoveryRequired
    );
    assert_eq!(tokens.calls.load(Ordering::SeqCst), 0);
    let HttpResponse::Ready(response) = rotated() else {
        unreachable!()
    };
    assert!(release.send(response).is_ok());
    assert_eq!(work.await.unwrap().unwrap(), RefreshOutcome::Refreshed);
    assert_eq!(
        world
            .http
            .calls()
            .iter()
            .filter(|c| c.path.ends_with("/refresh/v1"))
            .count(),
        1
    );
}

#[tokio::test]
async fn logout_failure_preserves_credentials_and_local_logout_clears_the_pair_and_generation() {
    let world = World::new([json!({"code":500}).into()], true).await;
    let before = world.session.token().unwrap();
    assert!(world.manager.logout(true).await.is_err());
    assert_eq!(world.session.token().unwrap(), before);
    assert!(
        world
            .credentials
            .login_credentials()
            .unwrap()
            .token
            .is_some()
    );
    let result = world.manager.logout(false).await.unwrap();
    assert!(result.local_signed_out);
    assert!(!result.remote_revoked);
    let credentials = world.credentials.login_credentials().unwrap();
    assert!(
        credentials.token.is_none()
            && credentials.refresh_token.is_none()
            && credentials.rotation.is_none()
    );
    assert_eq!(
        world.session.snapshot().unwrap().status,
        SessionStatus::SignedOut
    );
    assert_ne!(world.session.token().unwrap().generation, before.generation);
    assert_eq!(world.http.calls().len(), 2);
    assert!(
        world
            .store
            .get("1", "account", "profile")
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn legacy_refresh_expiry_and_changed_identity_do_not_publish_the_wrong_account() {
    let expired = World::new([json!({"code":301}).into()], true).await;
    assert!(
        expired
            .manager
            .refresh_legacy()
            .await
            .unwrap_err()
            .is_service_code(301)
    );
    assert_eq!(
        expired.session.snapshot().unwrap().status,
        SessionStatus::Expired
    );
    expired.manager.logout(false).await.unwrap();
    let changed = World::new(
        [
            json!({"code":200}).into(),
            json!({"code":200,"profile":{"userId":2}}).into(),
        ],
        true,
    )
    .await;
    assert!(matches!(
        changed.manager.refresh_legacy().await,
        Err(Error::Response(_))
    ));
    assert_eq!(
        changed.session.snapshot().unwrap().status,
        SessionStatus::Expired
    );
    assert!(
        changed
            .store
            .get("2", "account", "profile")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn invalid_device_or_malformed_provider_result_does_not_create_refresh_intent() {
    let world = World::new([], true).await;
    let tokens = Tokens::valid();
    assert!(
        world
            .manager
            .refresh_jsk(&DeviceInfo::default(), &tokens, true)
            .await
            .is_err()
    );
    assert_eq!(tokens.calls.load(Ordering::SeqCst), 0);
    let invalid = Tokens {
        calls: AtomicU64::new(0),
        value: "bad\nvalue",
    };
    assert!(
        world
            .manager
            .refresh_jsk(&world.device, &invalid, true)
            .await
            .is_err()
    );
    assert!(
        world
            .store
            .get("1", "login_refresh", "jsk")
            .unwrap()
            .is_none()
    );
    assert_eq!(world.http.calls().len(), 1);
}

#[tokio::test]
async fn local_credential_commit_failure_does_not_report_logout_success() {
    let world = World::new([], true).await;
    let before = world.session.token().unwrap();
    let db = rusqlite::Connection::open(world.directory.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_credentials BEFORE INSERT ON state WHEN NEW.kind='credentials' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END").unwrap();
    assert!(matches!(
        world.manager.logout(false).await,
        Err(Error::Storage(_))
    ));
    assert_eq!(world.session.token().unwrap(), before);
    assert!(
        world
            .credentials
            .login_credentials()
            .unwrap()
            .token
            .is_some()
    );
}
