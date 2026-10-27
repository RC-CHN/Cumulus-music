mod support;
use cumulus_core::{
    Error,
    account::{
        credentials::Credentials,
        refresh::{DesktopDeviceToken, DeviceInfo, DeviceTokenProvider},
    },
    client::{
        MusicClient,
        dto::{LoginCommand, LoginMode, RenewalAction as Action, RenewalState as State},
    },
    protocol::{
        api::ApiFuture,
        context::{RequestContext, TokenProvider},
        http::{ApiOrigin, ClientProfile},
    },
    storage::StateStore,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::http::{HttpResponse, ScriptHttp, gate, reply};

fn identity(uid: u64) -> HttpResponse {
    json!({"code":200,"profile":{"userId":uid,"nickname":format!("账号{uid}")}}).into()
}
fn rotated() -> HttpResponse {
    let mut r = reply(json!({"code":200}));
    r.set_cookies = vec!["MUSIC_U=synthetic-new; Path=/; Secure".into()];
    r.refresh_token = Some("synthetic-new-refresh".into());
    HttpResponse::Ready(Ok(r))
}
struct World {
    root: tempfile::TempDir,
    http: Arc<ScriptHttp>,
    client: Arc<MusicClient>,
    device_id: String,
}
impl World {
    async fn new(
        replies: impl IntoIterator<Item = HttpResponse>,
        refresh: bool,
        context: Option<Arc<RequestContext>>,
        provider: Arc<dyn DeviceTokenProvider>,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let credentials = Arc::new(
            Credentials::open(
                Arc::new(StateStore::open(root.path().join("profile")).unwrap()),
                ApiOrigin::Pc,
            )
            .unwrap(),
        );
        let target = ApiOrigin::Pc.url();
        let before = credentials.request_credentials(&target).unwrap();
        credentials
            .record_response(
                &target,
                &["MUSIC_U=synthetic-old; Path=/; Secure".into()],
                refresh.then_some("synthetic-old-refresh"),
                &before.revision,
            )
            .unwrap();
        let http = Arc::new(ScriptHttp::new([identity(1)].into_iter().chain(replies)));
        let client = Arc::new(
            MusicClient::open_with_request_context(
                &root.path().join("host"),
                Some(&root.path().join("profile")),
                ClientProfile::default(),
                Some(http.clone()),
                context,
            )
            .unwrap()
            .with_session_renewal(
                DeviceInfo {
                    device_name: "Real,model".into(),
                    device_id: "discard-other-account-id".into(),
                    os: "pc".into(),
                    ..Default::default()
                },
                provider,
            ),
        );
        client.restore_account().await.unwrap();
        Self {
            root,
            http,
            client,
            device_id: credentials.device_id().unwrap(),
        }
    }
    fn epoch(&self) -> String {
        self.client.account().unwrap().epoch
    }
    async fn check(&self) -> cumulus_core::Result<cumulus_core::client::dto::SessionRenewalView> {
        self.client
            .renew_session(&self.epoch(), Action::Check)
            .await
    }
    fn reopened(&self, http: Arc<ScriptHttp>) -> MusicClient {
        MusicClient::open_with_http_backend(
            &self.root.path().join("host"),
            Some(&self.root.path().join("profile")),
            http,
        )
        .unwrap()
        .with_session_renewal(
            DeviceInfo {
                os: "pc".into(),
                ..Default::default()
            },
            Arc::new(DesktopDeviceToken),
        )
    }
}

#[tokio::test]
async fn missing_refresh_never_calls_provider_or_network_and_exports_no_secrets() {
    struct Forbidden;
    impl DeviceTokenProvider for Forbidden {
        fn token(&self) -> ApiFuture<'_, String> {
            panic!("missing credentials must not reach provider")
        }
    }
    let w = World::new([], false, None, Arc::new(Forbidden)).await;
    let v = w.check().await.unwrap();
    assert_eq!(v.state, State::MissingCredentials);
    assert!(!v.can_check && !v.can_reconcile && !v.busy);
    assert_eq!(w.http.calls().len(), 1);
    let serialized = serde_json::to_string(&v).unwrap();
    for secret in ["MUSIC_U", "synthetic", "discard", "profile", "Real,model"] {
        assert!(!serialized.contains(secret));
    }
}

#[tokio::test]
async fn desktop_empty_token_rotates_once_and_daily_gate_survives_reopen() {
    let w = World::new(
        [rotated(), identity(1)],
        true,
        None,
        Arc::new(DesktopDeviceToken),
    )
    .await;
    let result = w.check().await.unwrap();
    assert_eq!(result.state, State::Refreshed);
    assert!(result.last_success.is_some());
    let calls = w.http.calls();
    assert_eq!(calls[1].path, "/api/login/token/refresh/v1");
    assert_eq!(calls[1].payload["ydDeviceToken"], "");
    assert_eq!(calls[1].payload["fromPage"], "JSK");
    assert_eq!(calls[1].login_chain.as_deref(), Some(""));
    let sign = cumulus_core::account::refresh::encrypt_params(&json!({"deviceId":w.device_id,"model":"Real.model","os":"pc","refreshToken":"synthetic-old-refresh","token":"synthetic-old","userId":"1"})).unwrap();
    assert_eq!(calls[1].payload["_sign"], sign);
    assert_eq!(w.check().await.unwrap().state, State::NotDue);
    let http = Arc::new(ScriptHttp::new([identity(1)]));
    let reopened = w.reopened(http.clone());
    let account = reopened.restore_account().await.unwrap();
    let status = reopened
        .renew_session(&account.epoch, Action::Check)
        .await
        .unwrap();
    assert_eq!(status.state, State::NotDue);
    assert_eq!(status.last_success, result.last_success);
    assert_eq!(http.calls().len(), 1);
}

#[tokio::test]
async fn unknown_transport_outcome_is_durable_and_never_replays_rotation() {
    let w = World::new(
        [HttpResponse::Ready(Err(Error::Transport(
            "synthetic lost response".into(),
        )))],
        true,
        None,
        Arc::new(DesktopDeviceToken),
    )
    .await;
    assert!(w.check().await.is_err());
    assert_eq!(
        w.client.session_renewal(&w.epoch()).unwrap().state,
        State::RecoveryRequired
    );
    assert_eq!(
        w.check().await.unwrap().state,
        State::ReauthenticationRequired
    );
    let http = Arc::new(ScriptHttp::new([identity(1)]));
    let client = w.reopened(http.clone());
    let account = client.restore_account().await.unwrap();
    assert_eq!(
        client.session_renewal(&account.epoch).unwrap().state,
        State::RecoveryRequired
    );
    assert_eq!(
        client
            .renew_session(&account.epoch, Action::Reconcile)
            .await
            .unwrap()
            .state,
        State::ReauthenticationRequired
    );
    assert_eq!(http.calls().len(), 1);
    assert_eq!(w.http.calls().len(), 2);
}

#[tokio::test]
async fn identity_read_failure_recovers_committed_pair_without_replaying_refresh() {
    let w = World::new(
        [
            rotated(),
            HttpResponse::Ready(Err(Error::Transport("identity disconnected".into()))),
            identity(1),
        ],
        true,
        None,
        Arc::new(DesktopDeviceToken),
    )
    .await;
    assert!(w.check().await.is_err());
    assert_eq!(
        w.client.session_renewal(&w.epoch()).unwrap().state,
        State::RecoveryRequired
    );
    assert_eq!(w.check().await.unwrap().state, State::Refreshed);
    assert_eq!(
        w.http
            .calls()
            .iter()
            .filter(|c| c.path == "/api/login/token/refresh/v1")
            .count(),
        1
    );
}

#[tokio::test]
async fn pre_transport_context_failure_releases_own_intent_for_later_check() {
    struct Once(AtomicUsize);
    impl TokenProvider for Once {
        fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
            Box::pin(async move {
                if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(Error::State("synthetic provider offline"))
                } else {
                    Ok("synthetic-request-context-token".into())
                }
            })
        }
    }
    let context = Arc::new(RequestContext::new(
        json!({"yidun#paths":{"whiteList":[{"businessId":"fixture","list":["/api/login/token/refresh/v1"]}]}}),
        ApiOrigin::Pc.as_str().into(),
        Arc::new(Once(AtomicUsize::new(0))),
    ));
    let w = World::new(
        [rotated(), identity(1)],
        true,
        Some(context),
        Arc::new(DesktopDeviceToken),
    )
    .await;
    assert!(matches!(w.check().await, Err(Error::RequestNotSent(_))));
    let v = w.client.session_renewal(&w.epoch()).unwrap();
    assert_eq!(v.state, State::Failed);
    assert!(v.can_check && !v.can_reconcile);
    assert_eq!(w.http.calls().len(), 1);
    assert_eq!(w.check().await.unwrap().state, State::Refreshed);
    assert_eq!(w.http.calls().len(), 3);
}

#[tokio::test]
async fn duplicate_checks_and_aborted_request_preserve_single_flight_and_recovery() {
    let (response, mut sender) = gate();
    let w = World::new([response], true, None, Arc::new(DesktopDeviceToken)).await;
    let client = w.client.clone();
    let epoch = w.epoch();
    let task = tokio::spawn(async move { client.renew_session(&epoch, Action::Check).await });
    w.http.wait(2).await;
    let v = w.client.session_renewal(&w.epoch()).unwrap();
    assert!(v.busy);
    assert_eq!(v.state, State::Checking);
    assert!(matches!(w.check().await, Err(Error::MutationBusy)));
    task.abort();
    let _ = task.await;
    sender.closed().await;
    let v = w.client.session_renewal(&w.epoch()).unwrap();
    assert!(!v.busy);
    assert_eq!(v.state, State::RecoveryRequired);
    assert_eq!(
        w.check().await.unwrap().state,
        State::ReauthenticationRequired
    );
    assert_eq!(w.http.calls().len(), 2);
}

#[tokio::test]
async fn switching_accounts_during_refresh_does_not_publish_old_account_or_hold_switch_gate() {
    let (response, sender) = gate();
    let w = World::new(
        [response, rotated(), identity(2)],
        true,
        None,
        Arc::new(DesktopDeviceToken),
    )
    .await;
    let client = w.client.clone();
    let epoch = w.epoch();
    let old_epoch = epoch.clone();
    let task = tokio::spawn(async move { client.renew_session(&epoch, Action::Check).await });
    w.http.wait(2).await;
    let flow = w
        .client
        .begin_login(&w.epoch(), LoginMode::Email, None)
        .await
        .unwrap();
    let login = w
        .client
        .login_command(
            &flow.operation_id,
            LoginCommand::Email {
                address: "other@example.test".into(),
                password: "synthetic".into(),
                remember: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        login.status,
        cumulus_core::account::login::LoginStatus::Authenticated
    );
    let HttpResponse::Ready(r) = rotated() else {
        unreachable!()
    };
    assert!(sender.send(r).is_ok());
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert_ne!(w.epoch(), old_epoch);
    assert_eq!(
        w.client.account().unwrap().nickname.as_deref(),
        Some("账号2")
    );
    assert_eq!(
        w.client.session_renewal(&w.epoch()).unwrap().state,
        State::Ready
    );
    assert!(matches!(
        w.client.session_renewal(&old_epoch),
        Err(Error::StaleResponse)
    ));
    assert_eq!(w.http.calls().len(), 4);
}

#[tokio::test]
async fn login_in_progress_defers_refresh_and_local_logout_stops_it() {
    let w = World::new([], true, None, Arc::new(DesktopDeviceToken)).await;
    let flow = w
        .client
        .begin_login(&w.epoch(), LoginMode::Email, None)
        .await
        .unwrap();
    assert_eq!(w.check().await.unwrap().state, State::LoginInProgress);
    w.client.cancel_login(&flow.operation_id).unwrap();
    assert_eq!(
        w.client.session_renewal(&w.epoch()).unwrap().state,
        State::Ready
    );
    w.client.logout_account(&w.epoch(), false).await.unwrap();
    assert_eq!(w.check().await.unwrap().state, State::SignedOut);
    assert_eq!(w.http.calls().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn renewal_deadline_releases_flight_but_preserves_unknown_write() {
    let (response, mut sender) = gate();
    let w = World::new([response], true, None, Arc::new(DesktopDeviceToken)).await;
    let client = w.client.clone();
    let epoch = w.epoch();
    let task = tokio::spawn(async move { client.renew_session(&epoch, Action::Check).await });
    w.http.wait(2).await;
    tokio::time::advance(std::time::Duration::from_secs(46)).await;
    assert!(matches!(task.await.unwrap(), Err(Error::Transport(_))));
    sender.closed().await;
    let status = w.client.session_renewal(&w.epoch()).unwrap();
    assert!(!status.busy);
    assert_eq!(status.state, State::RecoveryRequired);
    assert_eq!(
        w.check().await.unwrap().state,
        State::ReauthenticationRequired
    );
    assert_eq!(w.http.calls().len(), 2);
}
