#[path = "client_login_policy/legacy.rs"]
mod legacy;
mod support;
use cumulus_core::{
    Error,
    account::{
        SessionStatus,
        credentials::Credentials,
        refresh::{DesktopDeviceToken, DeviceInfo},
    },
    client::{
        MusicClient,
        dto::{LoginCommand, LoginMode, RenewalAction, RenewalState},
    },
    protocol::http::ApiOrigin,
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::http::{HttpResponse, ScriptHttp, gate, reply};

fn identity(uid: u64) -> HttpResponse {
    json!({"code":200,"profile":{"userId":uid,"nickname":format!("账号{uid}")}}).into()
}
fn flags(jsk: &str, benefit: &str) -> HttpResponse {
    json!({"code":200,"data":[
        {"expName":"PH-PC-loginJsk","expGroupName":jsk},
        {"expName":"PH-PC-newLoginBenefit","expGroupName":benefit}
    ]})
    .into()
}
fn rejected() -> HttpResponse {
    json!({"code":301}).into()
}
fn validation(action: Value) -> HttpResponse {
    json!({"code":200,"data":{"action":action,"title":"需要重新登录","message":"请确认当前账号"}})
        .into()
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
}
impl World {
    async fn new(responses: impl IntoIterator<Item = HttpResponse>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path().join("profile")).unwrap());
        let credentials = Credentials::open(store.clone(), ApiOrigin::Pc).unwrap();
        let target = ApiOrigin::Pc.url();
        let revision = credentials.request_credentials(&target).unwrap().revision;
        credentials
            .record_response(
                &target,
                &["MUSIC_U=synthetic-old; Path=/; Secure".into()],
                Some("synthetic-old-refresh"),
                &revision,
            )
            .unwrap();
        store
            .put("1", "test-resource", "keep", &json!({"keep":true}))
            .unwrap();
        let http = Arc::new(ScriptHttp::new([identity(1)].into_iter().chain(responses)));
        let client = Arc::new(Self::open(root.path(), http.clone()));
        client.restore_account().await.unwrap();
        Self { root, http, client }
    }
    fn open(root: &std::path::Path, http: Arc<ScriptHttp>) -> MusicClient {
        MusicClient::open_with_http_backend(&root.join("host"), Some(&root.join("profile")), http)
            .unwrap()
            .with_official_login_policy()
            .with_session_renewal(DeviceInfo::default(), Arc::new(DesktopDeviceToken))
    }
    fn epoch(&self) -> String {
        self.client.account().unwrap().epoch
    }
    fn credentials(&self) -> Credentials {
        Credentials::open(
            Arc::new(StateStore::open(self.root.path().join("profile")).unwrap()),
            ApiOrigin::Pc,
        )
        .unwrap()
    }
    fn record(&self) -> Value {
        StateStore::open(self.root.path().join("profile"))
            .unwrap()
            .get("transport", "credentials", "session")
            .unwrap()
            .unwrap()
    }
}

#[tokio::test]
async fn valid_token_keeps_account_caches_experiments_and_never_replays_failed_request() {
    let w = World::new([
        rejected(),
        flags("t1", "t1"),
        validation(json!(1)),
        rejected(),
    ])
    .await;
    let epoch = w.epoch();
    for _ in 0..2 {
        assert!(matches!(
            w.client.daily_songs().await,
            Err(Error::LoginDeferred)
        ));
    }
    assert_eq!(w.epoch(), epoch);
    assert_eq!(
        w.client.account().unwrap().status,
        SessionStatus::Authenticated
    );
    let calls = w.http.calls();
    assert_eq!(calls.len(), 5);
    assert_eq!(
        calls
            .iter()
            .filter(|c| c.path == "/api/rtrs/abt/front/expinfo/list")
            .count(),
        1
    );
    let middle: Vec<_> = calls
        .iter()
        .filter(|c| c.path == "/api/middle/account/token/refresh")
        .collect();
    assert_eq!(middle.len(), 1);
    assert_eq!(middle[0].payload["fromAPI"], calls[1].path);
    assert_eq!(middle[0].payload["fromPage"], "JSK");
    assert_eq!(middle[0].login_chain.as_deref(), Some(""));
    let header: Value =
        serde_json::from_str(middle[0].payload["header"].as_str().unwrap()).unwrap();
    assert_eq!(header["x-login-chain-id"], "");
    let original: Value =
        serde_json::from_str(include_str!("fixtures/login_interception.json")).unwrap();
    assert_eq!(
        middle[0].payload["fromPage"],
        original["jskRequests"][1]["options"]["data"]["fromPage"]
    );
    assert_eq!(
        header["x-login-chain-id"],
        original["jskRequests"][1]["options"]["header"]["x-login-chain-id"]
    );
    assert!(calls[1].payload.get("fromPage").is_none());
    assert!(calls[1].login_chain.is_none());
    assert!(w.record().get("login_notice").is_none());
}

#[tokio::test]
async fn explicit_expiry_is_persisted_with_credentials_cleared_and_restart_makes_no_request() {
    let w = World::new([rejected(), flags("t1", "t1"), validation(json!(2))]).await;
    let mut events = w.client.subscribe_account().unwrap();
    let epoch = w.epoch();
    assert!(
        w.client
            .daily_songs()
            .await
            .err()
            .expect("request must fail")
            .is_service_code(301)
    );
    events.changed().await.unwrap();
    assert_ne!(events.borrow().generation.to_string(), epoch);
    let account = w.client.account().unwrap();
    assert_eq!(account.status, SessionStatus::Expired);
    assert_eq!(account.login_notice.unwrap().message, "请确认当前账号");
    let pair = w.credentials().login_credentials().unwrap();
    assert!(pair.token.is_none() && pair.refresh_token.is_none());
    let store = StateStore::open(w.root.path().join("profile")).unwrap();
    assert_eq!(
        store.get("1", "test-resource", "keep").unwrap(),
        Some(json!({"keep":true}))
    );
    let network = Arc::new(ScriptHttp::new([]));
    let reopened = World::open(w.root.path(), network.clone());
    assert_eq!(
        reopened.restore_account().await.unwrap().status,
        SessionStatus::Expired
    );
    assert!(network.calls().is_empty());
    assert_eq!(w.http.calls().len(), 4);
}

#[tokio::test]
async fn pc_bridge_converts_transport_failures_to_body_failures_and_expires_after_three() {
    for transport in [false, true] {
        let mut responses = vec![rejected(), flags("t1", "t1")];
        for index in 0..3 {
            if index > 0 {
                responses.push(rejected());
            }
            responses.push(if transport {
                HttpResponse::Ready(Err(Error::Transport("synthetic".into())))
            } else {
                json!({"code":500}).into()
            });
        }
        let w = World::new(responses).await;
        for index in 0..3 {
            let result = w.client.daily_songs().await;
            if index == 2 {
                assert!(
                    result
                        .err()
                        .expect("request must fail")
                        .is_service_code(301)
                );
            } else {
                assert!(matches!(result, Err(Error::LoginDeferred)));
            }
        }
        assert_eq!(w.client.account().unwrap().status, SessionStatus::Expired);
        assert_eq!(w.http.calls().len(), 8);
    }
}

#[tokio::test]
async fn pc_bridge_http_error_body_counts_as_a_business_rejection() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/login_interception.json")).unwrap();
    let case = &fixture["bridgeCases"][1];
    assert_eq!(case["body"]["code"], 301);
    let mut responses = vec![rejected(), flags("t1", "t1")];
    for index in 0..3 {
        if index > 0 {
            responses.push(rejected());
        }
        let mut response = reply(case["body"].clone());
        response.status = case["error"]["status"].as_u64().unwrap() as u16;
        responses.push(HttpResponse::Ready(Ok(response)));
    }
    let w = World::new(responses).await;
    for _ in 0..3 {
        assert!(w.client.daily_songs().await.is_err());
    }
    assert_eq!(w.client.account().unwrap().status, SessionStatus::Expired);
    assert_eq!(w.http.calls().len(), 8);
}

#[tokio::test]
async fn http_status_with_business_301_preserves_original_endpoint_and_unknown_actions_keep_account()
 {
    for action in [Value::Null, json!("2"), json!(3)] {
        let mut r = reply(json!({"code":301}));
        r.status = 401;
        let w = World::new([
            HttpResponse::Ready(Ok(r)),
            flags("t1", "t1"),
            validation(action),
        ])
        .await;
        assert!(matches!(
            w.client.daily_songs().await,
            Err(Error::LoginDeferred)
        ));
        assert_eq!(
            w.client.account().unwrap().status,
            SessionStatus::Authenticated
        );
        let calls = w.http.calls();
        assert_eq!(calls[3].payload["fromAPI"], calls[1].path);
    }
}

#[tokio::test]
async fn renewal_requires_both_server_assignments_and_missing_experiment_never_rotates() {
    for (jsk, benefit) in [("t1", "c"), ("c", "t1"), ("", ""), ("t1", "t1")] {
        let enabled = jsk == "t1" && benefit == "t1";
        let mut responses = vec![flags(jsk, benefit)];
        if enabled {
            responses.extend([rotated(), identity(1)]);
        }
        let w = World::new(responses).await;
        let result = w
            .client
            .renew_session(&w.epoch(), RenewalAction::Check)
            .await
            .unwrap();
        assert_eq!(
            result.state,
            if enabled {
                RenewalState::Refreshed
            } else {
                RenewalState::PolicyDisabled
            }
        );
        assert_eq!(w.http.calls().len(), if enabled { 4 } else { 2 });
        w.client
            .refresh_login_experiments(&w.epoch())
            .await
            .unwrap();
        assert_eq!(w.http.calls().len(), if enabled { 4 } else { 2 });
    }
}

#[tokio::test]
async fn late_invalid_notice_after_account_switch_cannot_clear_old_or_new_credentials() {
    let (pending, sender) = gate();
    let w = World::new([
        rejected(),
        flags("t1", "t1"),
        pending,
        rotated(),
        identity(2),
        flags("c", "c"),
    ])
    .await;
    let client = w.client.clone();
    let task = tokio::spawn(async move { client.daily_songs().await });
    w.http.wait(4).await;
    let flow = w
        .client
        .begin_login(&w.epoch(), LoginMode::Email, None)
        .await
        .unwrap();
    w.client
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
    let HttpResponse::Ready(response) = validation(json!(2)) else {
        unreachable!()
    };
    assert!(sender.send(response).is_ok());
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        w.client.account().unwrap().nickname.as_deref(),
        Some("账号2")
    );
    assert!(w.credentials().login_credentials().unwrap().token.is_some());
    w.client
        .refresh_login_experiments(&w.epoch())
        .await
        .unwrap();
    assert_eq!(
        w.client.session_renewal(&w.epoch()).unwrap().state,
        RenewalState::PolicyDisabled
    );
    assert_eq!(
        w.http
            .calls()
            .iter()
            .filter(|c| c.path == "/api/rtrs/abt/front/expinfo/list")
            .count(),
        2
    );
}

#[tokio::test]
async fn failed_invalidation_commit_expires_memory_but_reports_error_and_allows_local_cleanup_retry()
 {
    let w = World::new([rejected(), flags("t1", "t1"), validation(json!(2))]).await;
    let before = w.record();
    let db = rusqlite::Connection::open(w.root.path().join("profile/core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_write BEFORE INSERT ON state WHEN NEW.kind='credentials' BEGIN SELECT RAISE(ABORT,'synthetic full disk'); END;").unwrap();
    assert!(matches!(
        w.client.daily_songs().await,
        Err(Error::Storage(_))
    ));
    assert_eq!(w.record(), before);
    assert_eq!(w.client.account().unwrap().status, SessionStatus::Expired);
    assert!(w.client.account().unwrap().can_sign_out);
    db.execute_batch("DROP TRIGGER reject_write").unwrap();
    let done = w.client.logout_account(&w.epoch(), false).await.unwrap();
    assert_eq!(done.account.status, SessionStatus::SignedOut);
    assert!(done.account.login_notice.is_none());
    assert!(w.credentials().login_credentials().unwrap().token.is_none());
    assert_eq!(w.http.calls().len(), 4);
}

#[tokio::test]
async fn experiment_groups_survive_reopen_and_failed_read_without_rotating_or_changing_account() {
    let w = World::new([flags("t1", "t1")]).await;
    w.client
        .refresh_login_experiments(&w.epoch())
        .await
        .unwrap();
    let store = StateStore::open(w.root.path().join("profile")).unwrap();
    assert_eq!(
        store.get("1", "login-policy-v1", "pc").unwrap(),
        Some(json!({"jsk":true,"benefit":true}))
    );
    let http = Arc::new(ScriptHttp::new([
        identity(1),
        HttpResponse::Ready(Err(Error::Transport("synthetic offline".into()))),
    ]));
    let client = World::open(w.root.path(), http.clone());
    let account = client.restore_account().await.unwrap();
    client
        .refresh_login_experiments(&account.epoch)
        .await
        .unwrap();
    assert_eq!(
        client.session_renewal(&account.epoch).unwrap().state,
        RenewalState::Ready
    );
    client
        .refresh_login_experiments(&account.epoch)
        .await
        .unwrap();
    assert_eq!(http.calls().len(), 2);
    assert_eq!(
        client.account().unwrap().status,
        SessionStatus::Authenticated
    );
}

#[tokio::test]
async fn experiment_cache_write_failure_preserves_previous_record_and_cannot_enable_rotation() {
    let w = World::new([flags("t1", "t1")]).await;
    let store = StateStore::open(w.root.path().join("profile")).unwrap();
    store
        .put(
            "1",
            "login-policy-v1",
            "pc",
            &json!({"jsk":false,"benefit":false}),
        )
        .unwrap();
    let db = rusqlite::Connection::open(w.root.path().join("profile/core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_policy BEFORE INSERT ON state WHEN NEW.kind='login-policy-v1' BEGIN SELECT RAISE(ABORT,'synthetic full disk'); END;").unwrap();
    assert!(matches!(
        w.client
            .renew_session(&w.epoch(), RenewalAction::Check)
            .await,
        Err(Error::Storage(_))
    ));
    assert_eq!(
        store.get("1", "login-policy-v1", "pc").unwrap(),
        Some(json!({"jsk":false,"benefit":false}))
    );
    assert_eq!(w.http.calls().len(), 2);
}
