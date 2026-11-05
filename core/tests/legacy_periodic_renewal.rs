mod support;
use cumulus_core::{
    Error,
    account::{
        QrStatus, SessionStatus,
        credentials::Credentials,
        refresh::{DesktopDeviceToken, DeviceInfo},
    },
    client::{
        MusicClient,
        dto::{RenewalAction, RenewalState},
    },
    protocol::http::ApiOrigin,
    storage::StateStore,
};
use serde_json::json;
use std::{path::Path, sync::Arc, time::Duration};
use support::http::{HttpResponse, ScriptHttp, gate, reply};

const PATH: &str = "/api/login/token/refresh";
const IDENTITY: &str = "/api/w/nuser/account/get";
const ASSIGNMENT: &str = "/api/rtrs/abt/front/expinfo/list";
const PERIOD: Duration = Duration::from_secs(72_000);
const AFTER_FAILURE: Duration = Duration::from_secs(3_603);
const WAIT: Duration = Duration::from_secs(3);
const MS: Duration = Duration::from_millis(1);

fn identity() -> HttpResponse {
    json!({"code":200,"account":{"id":42},"profile":{"userId":42,"nickname":"合成账号"}}).into()
}
fn experiments(group: &str) -> HttpResponse {
    json!({"code":200,"data":[
        {"expName":"PH-PC-loginJsk","expGroupName":group},
        {"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"},
    ]})
    .into()
}
fn build(root: &Path, http: Arc<ScriptHttp>, restore: bool) -> MusicClient {
    MusicClient::open_with_http_backend(
        root,
        restore.then_some(root.join("profile").as_path()),
        http,
    )
    .unwrap()
    .with_official_login_policy()
    .with_session_renewal(DeviceInfo::default(), Arc::new(DesktopDeviceToken))
}
struct World {
    root: tempfile::TempDir,
    client: MusicClient,
    http: Arc<ScriptHttp>,
    store: Arc<StateStore>,
}
impl World {
    async fn cold(replies: impl IntoIterator<Item = HttpResponse>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path().join("profile")).unwrap());
        let credentials = Credentials::open(store.clone(), ApiOrigin::Pc).unwrap();
        let target = ApiOrigin::Pc.url();
        let before = credentials.request_credentials(&target).unwrap();
        credentials
            .record_response(
                &target,
                &["MUSIC_U=synthetic-old; Path=/; Secure".into()],
                Some("synthetic-old-rt"),
                &before.revision,
            )
            .unwrap();
        let http = Arc::new(ScriptHttp::new([identity()].into_iter().chain(replies)));
        let client = build(root.path(), http.clone(), true);
        client.restore_account().await.unwrap();
        Self {
            root,
            client,
            http,
            store,
        }
    }
    fn credentials(&self) -> Credentials {
        Credentials::open(self.store.clone(), ApiOrigin::Pc).unwrap()
    }
    fn record(&self) -> serde_json::Value {
        self.store
            .get("42", "login_refresh", "legacy-periodic")
            .unwrap()
            .unwrap()
    }
}
async fn step(time: Duration) {
    tokio::time::advance(time).await;
    tokio::task::yield_now().await;
}

#[tokio::test(start_paused = true)]
async fn cold_restore_waits_three_seconds_and_a_headerless_acknowledgement_keeps_existing_credentials()
 {
    let mut ack = reply(json!({"code":"200"}));
    ack.refresh_token = Some("ignored-legacy-response-rt".into());
    let world = World::cold([
        experiments("c"),
        HttpResponse::Ready(Ok(ack)),
        identity(),
        json!({"code":200}).into(),
        identity(),
    ])
    .await;
    let _worker = world.client.start_legacy_session_renewal().unwrap();
    assert!(matches!(
        world.client.start_legacy_session_renewal(),
        Err(Error::MutationBusy)
    ));
    world.http.wait(2).await;
    step(WAIT - MS).await;
    assert_eq!(world.http.calls().len(), 2);
    step(MS).await;
    world.http.wait(4).await;
    let calls = world.http.calls();
    assert_eq!(
        calls.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
        [IDENTITY, ASSIGNMENT, PATH, IDENTITY]
    );
    assert!(calls[2].payload.get("fromPage").is_none());
    assert!(calls[2].payload.get("fromAPI").is_none());
    assert!(calls[2].payload.get("refreshToken").is_none());
    assert!(calls[2].login_chain.is_none());
    assert!(
        calls[2]
            .cookie
            .as_deref()
            .unwrap()
            .contains("MUSIC_U=synthetic-old")
    );
    assert!(world.record()["pending"].is_null());
    assert!(world.record()["last_success"].as_u64().is_some());
    let credentials = world.credentials().login_credentials().unwrap();
    assert_eq!(credentials.token.as_deref(), Some("synthetic-old"));
    assert_eq!(
        credentials.refresh_token.as_deref(),
        Some("synthetic-old-rt")
    );
    assert!(credentials.last_login_at.is_none());
    step(PERIOD - MS).await;
    assert_eq!(world.http.calls().len(), 4);
    step(MS).await;
    world.http.wait(6).await;
}

#[tokio::test(start_paused = true)]
async fn ordinary_qr_adoption_waits_twenty_hours_plus_three_seconds() {
    let root = tempfile::tempdir().unwrap();
    let mut authorized = reply(json!({"code":803}));
    authorized.set_cookies = vec!["MUSIC_U=synthetic-qr; Path=/; Secure".into()];
    let http = Arc::new(ScriptHttp::new([
        experiments("c"),
        json!({"code":200,"unikey":"synthetic-qr-key"}).into(),
        HttpResponse::Ready(Ok(authorized)),
        identity(),
        experiments("c"),
        json!({"code":200}).into(),
        identity(),
    ]));
    let client = build(root.path(), http.clone(), false);
    let _worker = client.start_legacy_session_renewal().unwrap();
    step(PERIOD + WAIT).await;
    assert!(http.calls().is_empty());
    let qr = client.begin_qr().await.unwrap();
    assert_eq!(
        client.poll_qr(&qr.operation_id).await.unwrap(),
        QrStatus::Authorized
    );
    http.wait(5).await;
    step(PERIOD + WAIT - MS).await;
    assert_eq!(http.calls().len(), 5);
    step(MS).await;
    http.wait(7).await;
    assert_eq!(http.calls()[5].path, PATH);
}

#[tokio::test(start_paused = true)]
async fn explicit_rejection_retries_after_one_hour_and_replacement_cookie_is_not_a_new_jsk_pair() {
    let mut rejected = reply(json!({"code":301}));
    rejected.set_cookies = vec!["MUSIC_U=ignored-error; Path=/; Secure".into()];
    let mut rotated = reply(json!({"code":200}));
    rotated.set_cookies = vec!["MUSIC_U=legacy-replacement; Path=/; Secure".into()];
    rotated.refresh_token = Some("ignored-unproven-rt".into());
    let world = World::cold([
        experiments("c"),
        HttpResponse::Ready(Ok(rejected)),
        HttpResponse::Ready(Ok(rotated)),
        identity(),
    ])
    .await;
    let _worker = world.client.start_legacy_session_renewal().unwrap();
    world.http.wait(2).await;
    step(WAIT).await;
    world.http.wait(3).await;
    assert!(world.record()["last_success"].is_null());
    assert!(world.record()["pending"].is_null());
    step(AFTER_FAILURE - MS).await;
    assert_eq!(world.http.calls().len(), 3);
    step(MS).await;
    world.http.wait(5).await;
    let calls = world.http.calls();
    assert_eq!(calls[3].path, PATH);
    assert!(
        calls[3]
            .cookie
            .as_deref()
            .unwrap()
            .contains("MUSIC_U=synthetic-old")
    );
    assert!(
        calls[4]
            .cookie
            .as_deref()
            .unwrap()
            .contains("MUSIC_U=legacy-replacement")
    );
    let credentials = world.credentials().login_credentials().unwrap();
    assert_eq!(credentials.token.as_deref(), Some("legacy-replacement"));
    assert!(credentials.refresh_token.is_none());
    assert!(credentials.rotation.is_none());
}

#[tokio::test(start_paused = true)]
async fn jsks_assignment_or_failed_assignment_does_not_start_a_legacy_exchange() {
    for policy in [
        experiments("t1"),
        json!({"code":500}).into(),
        json!({"code":200,"data":[]}).into(),
        json!({"code":200,"data":[
            {"expName":"PH-PC-loginJsk","expGroupName":"c"},
            {"expName":"PH-PC-loginJsk","expGroupName":"t1"},
            {"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"},
        ]})
        .into(),
    ] {
        let world = World::cold([policy]).await;
        let _worker = world.client.start_legacy_session_renewal().unwrap();
        world.http.wait(2).await;
        step(PERIOD + WAIT).await;
        assert_eq!(world.http.calls().len(), 2);
        assert!(
            world
                .store
                .get("42", "login_refresh", "legacy-periodic")
                .unwrap()
                .is_none()
        );
    }
}

#[tokio::test(start_paused = true)]
async fn unknown_write_is_not_replayed_after_timeout_or_reopen_and_blocks_jsk() {
    let (blocked, mut sender) = gate();
    let world = World::cold([experiments("c"), blocked]).await;
    let worker = world.client.start_legacy_session_renewal().unwrap();
    world.http.wait(2).await;
    step(WAIT).await;
    world.http.wait(3).await;
    step(Duration::from_secs(45)).await;
    sender.closed().await;
    step(PERIOD + WAIT).await;
    assert_eq!(world.http.calls().len(), 3);
    assert!(world.record()["pending"].as_str().is_some());
    assert!(world.record()["last_success"].is_null());
    drop(worker);
    let reopened_http = Arc::new(ScriptHttp::new([identity()]));
    let reopened = build(world.root.path(), reopened_http.clone(), true);
    reopened.restore_account().await.unwrap();
    let epoch = reopened.account().unwrap().epoch;
    let view = reopened
        .renew_session(&epoch, RenewalAction::Check)
        .await
        .unwrap();
    assert_eq!(view.state, RenewalState::Unconfirmed);
    assert!(!view.can_reconcile);
    let _worker = reopened.start_legacy_session_renewal().unwrap();
    step(PERIOD + WAIT).await;
    assert_eq!(reopened_http.calls().len(), 1);
    assert_eq!(
        reopened.account().unwrap().status,
        SessionStatus::Authenticated
    );
}

#[tokio::test(start_paused = true)]
async fn signout_cancels_the_inflight_request_and_a_late_reply_cannot_publish() {
    let (blocked, mut sender) = gate();
    let world = World::cold([experiments("c"), blocked]).await;
    let _worker = world.client.start_legacy_session_renewal().unwrap();
    world.http.wait(2).await;
    step(WAIT).await;
    world.http.wait(3).await;
    let epoch = world.client.account().unwrap().epoch;
    world.client.logout_account(&epoch, false).await.unwrap();
    sender.closed().await;
    step(PERIOD + WAIT).await;
    assert_eq!(world.http.calls().len(), 3);
    assert!(world.record()["pending"].as_str().is_some());
    assert!(
        world
            .credentials()
            .login_credentials()
            .unwrap()
            .token
            .is_none()
    );
    assert_eq!(
        world.client.account().unwrap().status,
        SessionStatus::SignedOut
    );
}
