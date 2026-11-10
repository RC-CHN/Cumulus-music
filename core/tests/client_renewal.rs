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
    atomic::{AtomicU64, AtomicUsize, Ordering},
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
        Self::configured(replies, refresh, context, provider, false).await
    }
    async fn configured(
        replies: impl IntoIterator<Item = HttpResponse>,
        refresh: bool,
        context: Option<Arc<RequestContext>>,
        provider: Arc<dyn DeviceTokenProvider>,
        official_policy: bool,
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
        let client = MusicClient::open_with_request_context(
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
        );
        let client = Arc::new(if official_policy {
            client.with_official_login_policy()
        } else {
            client
        });
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
    fn credentials(&self) -> Credentials {
        Credentials::open(
            Arc::new(StateStore::open(self.root.path().join("profile")).unwrap()),
            ApiOrigin::Pc,
        )
        .unwrap()
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
    let success = result.last_success.unwrap();
    let calls = w.http.calls();
    assert_eq!(calls[1].path, "/api/login/token/refresh/v1");
    assert_eq!(calls[1].payload["ydDeviceToken"], "");
    assert_eq!(calls[1].payload["fromPage"], "JSK");
    assert_eq!(calls[1].login_chain.as_deref(), Some(""));
    let sign = cumulus_core::account::refresh::encrypt_params(&json!({"deviceId":w.device_id,"model":"Real.model","os":"pc","refreshToken":"synthetic-old-refresh","token":"synthetic-old","userId":"1"})).unwrap();
    assert_eq!(calls[1].payload["_sign"], sign);
    assert_eq!(w.check().await.unwrap().state, State::NotDue);
    let http = Arc::new(ScriptHttp::new([identity(1)]));
    let clock = Arc::new(AtomicU64::new(success + 86_399));
    let reopened = w.reopened(http.clone()).with_session_renewal_clock({
        let clock = clock.clone();
        Arc::new(move || Ok(clock.load(Ordering::SeqCst)))
    });
    let account = reopened.restore_account().await.unwrap();
    let before = reopened.session_renewal(&account.epoch).unwrap();
    assert_eq!(
        before.state,
        State::NotDue,
        "read the saved day before checking"
    );
    assert_eq!(before.last_success, Some(success));
    assert_eq!(http.calls().len(), 1);
    let status = reopened
        .renew_session(&account.epoch, Action::Check)
        .await
        .unwrap();
    assert_eq!(status.state, State::NotDue);
    assert_eq!(status.last_success, result.last_success);
    clock.store(success - 1, Ordering::SeqCst);
    assert_eq!(
        reopened.session_renewal(&account.epoch).unwrap().state,
        State::NotDue
    );
    clock.store(success + 86_400, Ordering::SeqCst);
    let due = reopened.session_renewal(&account.epoch).unwrap();
    assert_eq!(
        due.state,
        State::Ready,
        "an old NotDue result must not survive the daily boundary"
    );
    assert_eq!(due.last_success, Some(success));
    assert!(due.can_check && !due.can_reconcile);
    assert_eq!(http.calls().len(), 1);
}

#[tokio::test]
async fn fresh_jsk_login_projects_the_local_interval_before_any_renewal_check() {
    use cumulus_core::account::profiles::ProfileRegistry;

    let root = tempfile::tempdir().unwrap();
    let http = Arc::new(ScriptHttp::new([
        json!({"code":200,"data":[
            {"expName":"PH-PC-loginJsk","expGroupName":"t1"},
            {"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"}
        ]})
        .into(),
        rotated(),
        identity(2),
    ]));
    let clock = Arc::new(AtomicU64::new(0));
    let client = MusicClient::open_with_http_backend(root.path(), None, http.clone())
        .unwrap()
        .with_official_login_policy()
        .with_session_renewal(DeviceInfo::default(), Arc::new(DesktopDeviceToken))
        .with_session_renewal_clock({
            let clock = clock.clone();
            Arc::new(move || Ok(clock.load(Ordering::SeqCst)))
        });
    let epoch = client.account().unwrap().epoch;
    let flow = client
        .begin_login(&epoch, LoginMode::Email, None)
        .await
        .unwrap();
    client
        .login_command(
            &flow.operation_id,
            LoginCommand::Email {
                address: "fresh@example.test".into(),
                password: "synthetic".into(),
                remember: true,
            },
        )
        .await
        .unwrap();
    let at = ProfileRegistry::open(root.path(), ApiOrigin::Pc)
        .unwrap()
        .current(ClientProfile::default())
        .unwrap()
        .unwrap()
        .credentials
        .login_credentials()
        .unwrap()
        .last_login_at
        .unwrap();
    let epoch = client.account().unwrap().epoch;
    for (time, expected) in [
        (at, State::NotDue),
        (at - 1, State::NotDue),
        (at + 86_399, State::NotDue),
        (at + 86_400, State::Ready),
    ] {
        clock.store(time, Ordering::SeqCst);
        let view = client.session_renewal(&epoch).unwrap();
        assert_eq!(view.state, expected);
        assert!(
            view.last_success.is_none(),
            "a new login is not a successful renewal"
        );
        assert!(!serde_json::to_string(&view).unwrap().contains("lastLogin"));
    }
    assert_eq!(
        http.calls().len(),
        3,
        "the read-only view performs no network I/O"
    );
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
    let before = w.client.session_renewal(&w.epoch()).unwrap();
    assert_eq!(before.state, State::Unconfirmed);
    assert!(!before.can_check && !before.can_reconcile);
    assert_eq!(w.check().await.unwrap().state, State::Unconfirmed);
    let http = Arc::new(ScriptHttp::new([identity(1)]));
    let client = w.reopened(http.clone());
    let account = client.restore_account().await.unwrap();
    assert_eq!(
        client.session_renewal(&account.epoch).unwrap().state,
        State::Unconfirmed
    );
    assert_eq!(
        client
            .renew_session(&account.epoch, Action::Reconcile)
            .await
            .unwrap()
            .state,
        State::Unconfirmed
    );
    assert_eq!(
        account.status,
        cumulus_core::account::SessionStatus::Authenticated
    );
    assert_eq!(http.calls().len(), 1);
    assert_eq!(w.http.calls().len(), 2);
}

fn jsk_flags() -> HttpResponse {
    json!({"code":200,"data":[
        {"expName":"PH-PC-loginJsk","expGroupName":"t1"},
        {"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"}
    ]})
    .into()
}

#[tokio::test]
async fn incomplete_or_rejected_periodic_refresh_keeps_session_and_does_not_intercept_301() {
    use cumulus_core::account::SessionStatus;

    for code in [200, 301] {
        let w = World::configured(
            [jsk_flags(), json!({"code":code}).into()],
            true,
            None,
            Arc::new(DesktopDeviceToken),
            true,
        )
        .await;
        let epoch = w.epoch();
        let before = w.credentials().login_credentials().unwrap();
        let result = w.check().await;
        if code == 301 {
            assert!(
                result
                    .err()
                    .expect("periodic refresh refused")
                    .is_service_code(301)
            );
        } else {
            assert_eq!(result.unwrap().state, State::Unconfirmed);
        }
        let status = w.client.session_renewal(&epoch).unwrap();
        assert_eq!(status.state, State::Unconfirmed);
        assert!(!status.can_check && !status.can_reconcile && status.last_success.is_none());
        assert_eq!(
            w.client.account().unwrap().status,
            SessionStatus::Authenticated
        );
        assert!(w.client.account().unwrap().login_notice.is_none());
        assert_eq!(w.check().await.unwrap().state, State::Unconfirmed);
        let after = w.credentials().login_credentials().unwrap();
        assert_eq!(before.revision, after.revision);
        assert_eq!(before.token, after.token);
        assert_eq!(before.refresh_token, after.refresh_token);
        assert_eq!(
            w.http
                .calls()
                .iter()
                .map(|call| call.path.as_str())
                .collect::<Vec<_>>(),
            [
                "/api/w/nuser/account/get",
                "/api/rtrs/abt/front/expinfo/list",
                "/api/login/token/refresh/v1"
            ]
        );
    }
}

#[tokio::test]
async fn identity_read_301_recovers_committed_pair_without_intercept_or_replaying_refresh() {
    use cumulus_core::account::SessionStatus;

    let w = World::configured(
        [jsk_flags(), rotated(), json!({"code":301}).into()],
        true,
        None,
        Arc::new(DesktopDeviceToken),
        true,
    )
    .await;
    let epoch = w.epoch();
    assert!(
        w.check()
            .await
            .err()
            .expect("identity read refused")
            .is_service_code(301)
    );
    let pending = w.client.session_renewal(&epoch).unwrap();
    assert_eq!(pending.state, State::RecoveryRequired);
    assert!(pending.can_reconcile && !pending.can_check && pending.last_success.is_none());
    assert_eq!(
        w.client.account().unwrap().status,
        SessionStatus::Authenticated
    );
    let http = Arc::new(ScriptHttp::new([identity(1), identity(1)]));
    let reopened = w.reopened(http.clone()).with_official_login_policy();
    let account = reopened.restore_account().await.unwrap();
    let pending = reopened.session_renewal(&account.epoch).unwrap();
    assert_eq!(pending.state, State::RecoveryRequired);
    assert!(pending.can_reconcile);
    assert_eq!(
        http.calls().len(),
        1,
        "reopening only restored the active session"
    );
    let done = reopened
        .renew_session(&account.epoch, Action::Reconcile)
        .await
        .unwrap();
    assert_eq!(done.state, State::Refreshed);
    assert!(done.last_success.is_some());
    assert_eq!(
        http.calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        ["/api/w/nuser/account/get", "/api/w/nuser/account/get"]
    );
    assert_eq!(
        w.http
            .calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [
            "/api/w/nuser/account/get",
            "/api/rtrs/abt/front/expinfo/list",
            "/api/login/token/refresh/v1",
            "/api/w/nuser/account/get"
        ]
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
    assert_eq!(v.state, State::Unconfirmed);
    assert_eq!(w.check().await.unwrap().state, State::Unconfirmed);
    assert_eq!(w.http.calls().len(), 2);
}

async fn switch_to_second_account(w: &World) {
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
}

#[derive(Default)]
struct HeldRefreshContext {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

impl TokenProvider for HeldRefreshContext {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async move {
            self.entered.notify_one();
            self.release.notified().await;
            Ok("synthetic-held-context".into())
        })
    }
}

fn held_refresh_context(provider: Arc<HeldRefreshContext>) -> Arc<RequestContext> {
    Arc::new(RequestContext::new(
        json!({"yidun#paths":{"whiteList":[{"businessId":"fixture","list":["/api/login/token/refresh/v1"]}]}}),
        ApiOrigin::Pc.as_str().into(),
        provider,
    ))
}

fn renewal_record(w: &World) -> serde_json::Value {
    StateStore::open(w.root.path().join("profile"))
        .unwrap()
        .get("1", "login_refresh", "jsk")
        .unwrap()
        .unwrap()
}

fn spawn_check(
    w: &World,
) -> tokio::task::JoinHandle<cumulus_core::Result<cumulus_core::client::dto::SessionRenewalView>> {
    let client = w.client.clone();
    let epoch = w.epoch();
    tokio::spawn(async move { client.renew_session(&epoch, Action::Check).await })
}

#[tokio::test]
async fn switching_accounts_while_preparing_refresh_prevents_first_dispatch() {
    let provider = Arc::new(HeldRefreshContext::default());
    let w = World::new(
        [rotated(), identity(2)],
        true,
        Some(held_refresh_context(provider.clone())),
        Arc::new(DesktopDeviceToken),
    )
    .await;
    let old_revision = w.credentials().login_credentials().unwrap().revision;
    let task = spawn_check(&w);
    provider.entered.notified().await;
    assert_eq!(w.http.calls().len(), 1);
    assert_eq!(renewal_record(&w)["pending"], true);

    switch_to_second_account(&w).await;
    assert_eq!(
        w.credentials().login_credentials().unwrap().revision,
        old_revision
    );
    provider.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(renewal_record(&w)["pending"], false);
    assert!(renewal_record(&w)["last_success"].is_null());
    assert_eq!(
        w.client.account().unwrap().nickname.as_deref(),
        Some("账号2")
    );
    let calls = w.http.calls();
    assert_eq!(calls.len(), 3);
    assert!(
        calls
            .iter()
            .all(|c| c.path != "/api/login/token/refresh/v1")
    );
}

#[tokio::test]
async fn changed_cookie_scope_while_preparing_refresh_never_sends_and_releases_intent() {
    let provider = Arc::new(HeldRefreshContext::default());
    let w = World::new(
        [],
        true,
        Some(held_refresh_context(provider.clone())),
        Arc::new(DesktopDeviceToken),
    )
    .await;
    let task = spawn_check(&w);
    provider.entered.notified().await;
    let credentials = w.credentials();
    let revision = credentials.login_credentials().unwrap().revision;
    credentials
        .record_response(
            &ApiOrigin::Pc.url(),
            &["MUSIC_U=synthetic-other-path; Path=/eapi/w/nuser/account/get; Secure".into()],
            None,
            &revision,
        )
        .unwrap();
    provider.release.notify_one();
    assert!(
        matches!(task.await.unwrap(), Err(Error::RequestNotSent(error)) if matches!(*error, Error::CredentialChanged))
    );
    assert_eq!(renewal_record(&w)["pending"], false);
    assert_eq!(w.http.calls().len(), 1);
    assert!(matches!(w.client.account(), Err(Error::CredentialChanged)));

    // A fresh host owns the new revision but still cannot send a refresh
    // whose Cookie scope is ambiguous at the identity URL.
    let http = Arc::new(ScriptHttp::new([identity(1)]));
    let reopened = w.reopened(http.clone());
    let account = reopened.restore_account().await.unwrap();
    assert!(matches!(
        reopened.renew_session(&account.epoch, Action::Check).await,
        Err(Error::CredentialChanged)
    ));
    assert_eq!(http.calls().len(), 1);
    assert_eq!(http.calls()[0].path, "/api/w/nuser/account/get");
}

#[tokio::test(start_paused = true)]
async fn deadline_while_preparing_refresh_releases_only_unsent_intent() {
    let provider = Arc::new(HeldRefreshContext::default());
    let w = World::new(
        [],
        true,
        Some(held_refresh_context(provider.clone())),
        Arc::new(DesktopDeviceToken),
    )
    .await;
    let task = spawn_check(&w);
    provider.entered.notified().await;
    assert_eq!(renewal_record(&w)["pending"], true);
    tokio::time::advance(std::time::Duration::from_secs(46)).await;
    assert!(matches!(task.await.unwrap(), Err(Error::Transport(_))));
    assert_eq!(renewal_record(&w)["pending"], false);
    assert_eq!(w.http.calls().len(), 1);
    assert_eq!(
        w.client.session_renewal(&w.epoch()).unwrap().state,
        State::Failed
    );
}

#[tokio::test]
async fn rotated_pair_outside_identity_scope_does_not_verify_against_another_cookie() {
    let HttpResponse::Ready(Ok(mut response)) = rotated() else {
        unreachable!()
    };
    // The old root Cookie would authenticate account/get, while this new
    // narrower Cookie is usable only for the refresh URL.
    response.set_cookies = vec![
        "MUSIC_U=; Path=/; Max-Age=0; Secure".into(),
        "MUSIC_U=synthetic-new; Path=/eapi/login/token/refresh/v1; Secure".into(),
        "MUSIC_U=synthetic-old; Path=/eapi/w/nuser/account/get; Secure".into(),
    ];
    let w = World::new(
        [HttpResponse::Ready(Ok(response))],
        true,
        None,
        Arc::new(DesktopDeviceToken),
    )
    .await;
    assert!(matches!(w.check().await, Err(Error::CredentialChanged)));
    assert!(
        w.credentials()
            .login_credentials()
            .unwrap()
            .rotation
            .is_some()
    );
    assert_eq!(renewal_record(&w)["pending"], true);
    assert!(renewal_record(&w)["last_success"].is_null());
    let epoch = w.epoch();
    assert_eq!(
        w.client.session_renewal(&epoch).unwrap().state,
        State::RecoveryRequired
    );
    assert!(matches!(
        w.client.renew_session(&epoch, Action::Reconcile).await,
        Err(Error::CredentialChanged)
    ));
    let calls = w.http.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].path, "/api/login/token/refresh/v1");
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
    switch_to_second_account(&w).await;
    let HttpResponse::Ready(r) = rotated() else {
        unreachable!()
    };
    assert!(sender.send(r).is_ok());
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(renewal_record(&w)["pending"], true);
    assert!(renewal_record(&w)["last_success"].is_null());
    assert_eq!(
        w.credentials()
            .login_credentials()
            .unwrap()
            .token
            .as_deref(),
        Some("synthetic-new")
    );
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
    assert_eq!(status.state, State::Unconfirmed);
    assert_eq!(w.check().await.unwrap().state, State::Unconfirmed);
    assert_eq!(w.http.calls().len(), 2);
}
