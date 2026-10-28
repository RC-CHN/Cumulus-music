mod support;
use cumulus_core::{
    Error,
    account::{AccountSession, SessionToken},
    relay::*,
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

struct World {
    _root: tempfile::TempDir,
    store: Arc<StateStore>,
    api: Arc<ScriptApi>,
    session: Arc<AccountSession>,
    service: Arc<RelayService>,
    token: SessionToken,
}
impl World {
    async fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let api = Arc::new(ScriptApi::new(
            std::iter::once(json!({"code":200,"profile":{"userId":1}}).into()).chain(replies),
        ));
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session.refresh().await.unwrap();
        let token = session.token().unwrap();
        let service = Arc::new(RelayService::new(session.clone()).unwrap());
        Self {
            _root: root,
            store,
            api,
            session,
            service,
            token,
        }
    }
    async fn read(&self) -> RelaySnapshot {
        self.service.refresh_for(&self.token).await.unwrap()
    }
    fn pulls(&self) -> usize {
        self.api
            .calls()
            .iter()
            .filter(|c| c.path == PULL_PATH)
            .count()
    }
}
fn ab(group: &str) -> Reply {
    json!({"code":200,"data":[{"expName":"device_connect","expGroupName":group}]}).into()
}
fn config(enable: bool) -> Reply {
    json!({"code":200,"data":{"enable":enable,"radioProgressInterval":30,"snapshotSize":1000}})
        .into()
}
fn setting(enable: bool) -> Reply {
    json!({"code":200,"data":{"groups":[{"deviceSettingList":[{"enable":enable}]}]}}).into()
}
fn alert() -> Reply {
    json!({"code":200,"data":{"commonResourceList":[{"generalizedObject":{"deviceId":"remote-synthetic","deviceName":"Synthetic device","reconnectTitle":"Continue","opaqueField":"native-only"}}]}}).into()
}
fn pull() -> Value {
    json!({"code":200,"data":{"relayMode":"snapshot","resources":[{"id":"2","type":"voice"}],"currentPlayState":{"resourceId":"1","resourceType":"voice","progress":12500},"playMode":"random"}})
}
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn lost() -> Reply {
    Reply::Ready(Err(Error::Transport("synthetic lost response".into())))
}

#[tokio::test]
async fn eligibility_and_missing_setting_never_invent_available_devices() {
    let w = World::new([
        ab("control"),
        json!({"code":200,"data":{"groups":[]}}).into(),
    ])
    .await;
    let snapshot = w.read().await;
    assert_eq!(snapshot.availability, RelayAvailability::ExperimentExcluded);
    assert_eq!(snapshot.setting, None);
    assert!(snapshot.alert.is_none());
    assert_eq!(
        w.api
            .calls()
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        [
            "/api/w/nuser/account/get",
            "/api/rtrs/abt/front/expinfo/list",
            SETTING_PATH
        ]
    );
    assert!(
        w.service
            .pull_for(&w.token, &id(), &snapshot.revision)
            .await
            .is_err()
    );
    assert_eq!(w.pulls(), 0);
    let w = World::new([ab("t2"), config(false), setting(true)]).await;
    let snapshot = w.read().await;
    assert_eq!(snapshot.availability, RelayAvailability::ServerDisabled);
    assert_eq!(snapshot.config.radio_progress_interval, 30);
    assert!(!w.api.calls().iter().any(|c| c.path == ALERT_PATH));
}
#[tokio::test]
async fn stale_cache_cannot_survive_a_failed_refresh_or_later_server_disable() {
    let w = World::new([ab("t2"), config(true), setting(true), alert(), lost()]).await;
    let snapshot = w.read().await;
    assert!(w.service.refresh_for(&w.token).await.is_err());
    assert_eq!(
        w.service.snapshot_for(&w.token).unwrap().availability,
        RelayAvailability::Unchecked
    );
    assert!(
        w.service
            .pull_for(&w.token, &id(), &snapshot.revision)
            .await
            .is_err()
    );
    assert_eq!(w.pulls(), 0);
    let w = World::new([
        ab("t2"),
        config(true),
        setting(true),
        alert(),
        ab("t2"),
        config(false),
    ])
    .await;
    let snapshot = w.read().await;
    assert!(
        w.service
            .pull_for(&w.token, &id(), &snapshot.revision)
            .await
            .is_err()
    );
    assert_eq!(w.pulls(), 0);
    assert_eq!(
        w.service.snapshot_for(&w.token).unwrap().availability,
        RelayAvailability::Unchecked
    );
}
#[tokio::test]
async fn explicit_pull_persists_single_intent_and_native_plan_across_restart() {
    let w = World::new([
        ab("t2"),
        config(true),
        setting(true),
        alert(),
        ab("t2"),
        config(true),
        pull().into(),
    ])
    .await;
    let snapshot = w.read().await;
    let id = id();
    let intent = w
        .service
        .pull_for(&w.token, &id, &snapshot.revision)
        .await
        .unwrap();
    assert_eq!(intent.phase, PullPhase::Ready);
    assert_eq!(intent.plan.unwrap().position_ms, 12500);
    assert!(w.service.snapshot_for(&w.token).unwrap().alert.is_none());
    let reopened = RelayService::new(w.session.clone()).unwrap();
    assert_eq!(
        reopened
            .pull_for(&w.token, &id, &snapshot.revision)
            .await
            .unwrap()
            .phase,
        PullPhase::Ready
    );
    assert!(reopened.pull_for(&w.token, &id, "different").await.is_err());
    assert_eq!(w.pulls(), 1);
    let call = w
        .api
        .calls()
        .into_iter()
        .find(|c| c.path == PULL_PATH)
        .unwrap();
    assert_eq!(call.payload["targetDeviceId"], "remote-synthetic");
    assert_eq!(call.payload["opaqueField"], "native-only");
    assert!(
        !w.api
            .calls()
            .iter()
            .any(|c| [SETTING_UPDATE_PATH, SOURCE_PATH, STATE_PATH].contains(&c.path.as_str()))
    );
}
#[tokio::test]
async fn lost_pull_never_replays_and_unseen_intent_cancellation_fences_delayed_ipc() {
    let w = World::new([
        ab("t2"),
        config(true),
        setting(true),
        alert(),
        ab("t2"),
        config(true),
        lost(),
    ])
    .await;
    let snapshot = w.read().await;
    let id = id();
    let result = w
        .service
        .pull_for(&w.token, &id, &snapshot.revision)
        .await
        .unwrap();
    assert_eq!(result.phase, PullPhase::Uncertain);
    assert_eq!(
        w.service
            .pull_for(&w.token, &id, &snapshot.revision)
            .await
            .unwrap()
            .phase,
        PullPhase::Uncertain
    );
    assert_eq!(w.pulls(), 1);
    let cancelled = uuid::Uuid::new_v4().to_string();
    w.service
        .cancel_unseen_for(&w.token, &cancelled, &snapshot.revision)
        .unwrap();
    assert_eq!(
        w.service
            .pull_for(&w.token, &cancelled, &snapshot.revision)
            .await
            .unwrap()
            .phase,
        PullPhase::Cancelled
    );
    assert_eq!(w.pulls(), 1);
}
#[tokio::test]
async fn simultaneous_pull_is_locked_and_account_switch_never_applies_old_plan() {
    let (pending, release) = gate();
    let w = World::new([
        ab("t2"),
        config(true),
        setting(true),
        alert(),
        ab("t2"),
        config(true),
        pending,
    ])
    .await;
    let snapshot = w.read().await;
    let id = id();
    let service = w.service.clone();
    let token = w.token.clone();
    let revision = snapshot.revision.clone();
    let task_id = id.clone();
    let task = tokio::spawn(async move { service.pull_for(&token, &task_id, &revision).await });
    w.api.wait_for_calls(8).await;
    assert!(matches!(
        w.service.pull_for(&w.token, &id, &snapshot.revision).await,
        Err(Error::MutationBusy)
    ));
    let other = Arc::new(ScriptApi::new([
        json!({"code":200,"profile":{"userId":2}}).into()
    ]));
    w.session.attach(other).unwrap();
    w.session.refresh().await.unwrap();
    release.send(Ok(pull())).unwrap();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        w.store
            .get("1", "relay-pull-intents-v1", &id)
            .unwrap()
            .unwrap()["phase"],
        "uncertain"
    );
    let next = w.session.token().unwrap();
    assert!(w.service.intent_for(&next, &id).unwrap().is_none());
    assert_eq!(
        w.service.snapshot_for(&next).unwrap().availability,
        RelayAvailability::Unchecked
    );
}
#[tokio::test]
async fn dismissal_is_local_and_server_rejection_does_not_start_playback() {
    let w = World::new([ab("t2"), config(true), setting(true), alert()]).await;
    let snapshot = w.read().await;
    w.service.dismiss_for(&w.token, &snapshot.revision).unwrap();
    assert!(
        w.service
            .pull_for(&w.token, &id(), &snapshot.revision)
            .await
            .is_err()
    );
    assert_eq!(w.pulls(), 0);
    let w = World::new([
        ab("t2"),
        config(true),
        setting(true),
        alert(),
        ab("t2"),
        config(true),
        json!({"code":403}).into(),
    ])
    .await;
    let snapshot = w.read().await;
    let intent = w
        .service
        .pull_for(&w.token, &id(), &snapshot.revision)
        .await
        .unwrap();
    assert_eq!(intent.phase, PullPhase::Rejected);
    assert!(intent.plan.is_none());
    assert!(w.service.snapshot_for(&w.token).unwrap().alert.is_none());
}
#[tokio::test]
async fn experiment_first_match_and_empty_or_malformed_values_are_not_t2() {
    for (value, expected) in [
        (json!([]), Some(false)),
        (
            json!([{"expName":"device_connect","expGroupName":"control"},{"expName":"device_connect","expGroupName":"t2"}]),
            Some(false),
        ),
        (
            json!([{"expName":"device_connect","expGroupName":"t2"}]),
            Some(true),
        ),
        (Value::Null, None),
    ] {
        let mock = Arc::new(ScriptApi::new([json!({"code":200,"data":value}).into()]));
        assert_eq!(
            RelayApi::new(mock.clone()).experiment().await.ok(),
            expected
        );
        assert_eq!(
            mock.calls()[0].payload,
            json!({"expNames":"device_connect"})
        );
    }
}
