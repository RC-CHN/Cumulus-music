mod support;
use cumulus_core::{
    Error,
    account::{AccountSession, resources::MutationPhase},
    mv::{MvApi, MvDetail, MvService},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use support::{Reply, ScriptApi, gate};
fn detail(liked: bool) -> Value {
    json!({"code":200,"data":{"id":"123","resource":{"threadId":"R_MV_5_123","liked":liked,"likedCount":77,"data":{"id":"123","name":"Fixture","duration":120000,"relateSong":[]}}}})
}
fn failure() -> Reply {
    Reply::Ready(Err(Error::Transport("fixture secret".into())))
}
struct Fixture {
    _root: tempfile::TempDir,
    api: Arc<ScriptApi>,
    store: Arc<StateStore>,
    session: Arc<AccountSession>,
    service: MvService,
}
impl Fixture {
    fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let api = Arc::new(ScriptApi::new(replies));
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        let service = MvService::new(session.clone())
            .unwrap()
            .with_settle_delay(Duration::ZERO);
        Self {
            _root: root,
            api,
            store,
            session,
            service,
        }
    }
    fn writes(&self) -> usize {
        self.api
            .calls()
            .iter()
            .filter(|r| r.path.starts_with("/api/resource/"))
            .count()
    }
}
#[tokio::test]
async fn official_resource_like_and_related_privilege_parameters_are_exact() {
    let api = Arc::new(ScriptApi::new([
        json!({"code":200}).into(),
        json!({"code":200}).into(),
        json!({"code":200,"data":[{"id":"90071992547409931","pl":320000}]}).into(),
    ]));
    let mv = MvApi::new(api.clone());
    mv.like(&json!("123"), true).await.unwrap();
    mv.like(&json!("123"), false).await.unwrap();
    let privileges = mv
        .related_privileges(&["90071992547409931".into()])
        .await
        .unwrap();
    assert_eq!(privileges.len(), 1);
    let calls = api.calls();
    let official: Value =
        serde_json::from_str(include_str!("fixtures/mv-social-official.json")).unwrap();
    for (call, expected) in calls.iter().zip(official["calls"].as_array().unwrap()) {
        assert_eq!(call.path, expected["path"]);
        assert_eq!(call.payload, expected["payload"]);
    }
    assert_eq!(official["default_trial_mode"], -1);
    assert_eq!(calls[0].path, "/api/resource/like");
    assert_eq!(calls[1].path, "/api/resource/unlike");
    assert_eq!(calls[0].payload, json!({"threadId":"R_MV_5_123"}));
    assert_eq!(calls[1].payload, calls[0].payload);
    assert_eq!(calls[2].path, "/api/song/enhance/privilege");
    assert_eq!(
        calls[2].payload,
        json!({"ids":"[\"90071992547409931\"]","trialMode":-1})
    );
    assert!(calls.iter().all(|c| !c.cache));
    assert!(mv.like(&json!("../123"), true).await.is_err());
    for value in [Value::Null, json!("false")] {
        let mut input = detail(false);
        input["data"]["resource"]["liked"] = value;
        let parsed = MvDetail::from_official(&input["data"]);
        if input["data"]["resource"]["liked"].is_null() {
            assert!(!parsed.unwrap().liked_known)
        } else {
            assert!(parsed.is_err())
        }
    }
}
#[tokio::test]
async fn related_privileges_refresh_only_when_missing_preserve_existing_and_report_failure() {
    let mut data = detail(false);
    data["data"]["resource"]["data"]["relateSong"] = json!([{"id":"7","name":"Song"},{"id":"8","name":"Existing","privilege":{"id":"8","pl":128000}}]);
    let f = Fixture::new([
        data.clone().into(),
        json!({"code":200,"data":[{"id":"7","pl":320000}]}).into(),
        data.clone().into(),
        failure(),
    ]);
    let t = f.session.token().unwrap();
    let result = f
        .service
        .download_options_for(&t, &json!("123"))
        .await
        .unwrap();
    assert_eq!(result.related_songs[0]["privilege"]["pl"], 320000);
    assert_eq!(result.related_songs[1]["privilege"]["pl"], 128000);
    assert!(!result.related_privilege_failed);
    assert_eq!(
        f.api.calls()[1].payload,
        json!({"ids":"[\"7\",\"8\"]","trialMode":-1})
    );
    let result = f
        .service
        .download_options_for(&t, &json!("123"))
        .await
        .unwrap();
    assert!(result.related_privilege_failed);
    assert_eq!(result.related_songs.len(), 2);
    data["data"]["resource"]["data"]["relateSong"][0]["privilege"] = json!({"id":"7"});
    let f = Fixture::new([data.into()]);
    f.service
        .download_options_for(&f.session.token().unwrap(), &json!("123"))
        .await
        .unwrap();
    assert_eq!(f.api.calls().len(), 1);
}
#[tokio::test]
async fn likes_require_two_account_flags_not_global_counts_and_noop_does_not_write() {
    let f = Fixture::new([
        detail(false).into(),
        json!({"code":200}).into(),
        detail(true).into(),
        detail(true).into(),
        detail(true).into(),
    ]);
    let t = f.session.token().unwrap();
    let result = f.service.like_for(&t, "123", true).await.unwrap();
    assert_eq!(result.phase, MutationPhase::Confirmed);
    assert!(result.acknowledged);
    assert_eq!(result.observed.unwrap().count, 77);
    assert_eq!(f.writes(), 1);
    let cached = f.service.cached_for(&t, &json!("123")).unwrap().unwrap();
    assert!(cached.liked && cached.liked_known);
    let result = f.service.like_for(&t, "123", true).await.unwrap();
    assert_eq!(result.phase, MutationPhase::Unchanged);
    assert_eq!(f.writes(), 1);
}
#[tokio::test]
async fn unknown_write_reopens_and_reconciles_read_only_under_original_account() {
    let f = Fixture::new([
        detail(false).into(),
        failure(),
        detail(true).into(),
        detail(true).into(),
    ]);
    let t = f.session.token().unwrap();
    assert!(f.service.like_for(&t, "123", true).await.is_err());
    assert_eq!(
        f.service
            .like_operation_for(&t, "123")
            .unwrap()
            .unwrap()
            .phase,
        MutationPhase::Uncertain
    );
    assert!(matches!(
        f.service.like_for(&t, "123", true).await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(f.writes(), 1);
    let store = Arc::new(StateStore::open(f.store.directory()).unwrap());
    let new = Arc::new(AccountSession::new(f.api.clone(), Some(store)));
    new.adopt(f.api.clone(), json!({"userId":1}), &new.token().unwrap())
        .unwrap();
    let service = MvService::new(new.clone())
        .unwrap()
        .with_settle_delay(Duration::ZERO);
    assert_eq!(
        service
            .reconcile_like_for(&new.token().unwrap(), "123")
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    assert_eq!(f.writes(), 1);
    f.session
        .adopt(f.api.clone(), json!({"userId":2}), &t)
        .unwrap();
    assert!(
        f.service
            .like_operation_for(&f.session.token().unwrap(), "123")
            .unwrap()
            .is_none()
    );
    assert!(f.service.cached_for(&t, &json!("123")).is_err());
}
#[tokio::test]
async fn unchanged_reads_after_lost_write_and_known_rejection_do_not_claim_success() {
    let mut replies = vec![detail(false).into(), failure()];
    replies.extend((0..8).map(|_| detail(false).into()));
    let f = Fixture::new(replies);
    let t = f.session.token().unwrap();
    assert!(f.service.like_for(&t, "123", true).await.is_err());
    assert!(matches!(
        f.service.reconcile_like_for(&t, "123").await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(f.writes(), 1);
    let f = Fixture::new([detail(false).into(), json!({"code":403}).into()]);
    let t = f.session.token().unwrap();
    assert!(
        f.service
            .like_for(&t, "123", true)
            .await
            .err()
            .unwrap()
            .is_service_code(403)
    );
    assert_eq!(
        f.service
            .like_operation_for(&t, "123")
            .unwrap()
            .unwrap()
            .phase,
        MutationPhase::Rejected
    );
    assert_eq!(f.writes(), 1);
}
#[tokio::test]
async fn cancelled_write_retains_intent_excludes_concurrent_write_and_late_owner_change() {
    let (hold, send) = gate();
    let f = Fixture::new([detail(false).into(), hold]);
    let session = f.session.clone();
    let t = session.token().unwrap();
    let old = t.clone();
    let job = tokio::spawn(async move {
        MvService::new(session)
            .unwrap()
            .like_for(&old, "123", true)
            .await
    });
    f.api.wait_for_calls(2).await;
    assert!(matches!(
        f.service.like_for(&t, "123", true).await,
        Err(Error::MutationBusy)
    ));
    f.session
        .adopt(f.api.clone(), json!({"userId":2}), &t)
        .unwrap();
    send.send(Ok(json!({"code":200}))).unwrap();
    assert!(matches!(job.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        f.store.get("1", "mv-likes-v1", "123").unwrap().unwrap()["phase"],
        "uncertain"
    );
    assert!(f.store.get("2", "mv-likes-v1", "123").unwrap().is_none());
    let (hold, _send) = gate();
    let f = Fixture::new([detail(false).into(), hold]);
    let session = f.session.clone();
    let old = session.token().unwrap();
    let job = tokio::spawn(async move {
        MvService::new(session)
            .unwrap()
            .like_for(&old, "123", true)
            .await
    });
    f.api.wait_for_calls(2).await;
    job.abort();
    assert!(job.await.err().unwrap().is_cancelled());
    assert_eq!(
        f.service
            .like_operation_for(&f.session.token().unwrap(), "123")
            .unwrap()
            .unwrap()
            .phase,
        MutationPhase::Sending
    );
}
#[tokio::test]
async fn older_detail_response_cannot_overwrite_confirmed_like_cache() {
    let (hold, send) = gate();
    let f = Fixture::new([
        hold,
        detail(false).into(),
        json!({"code":200}).into(),
        detail(true).into(),
        detail(true).into(),
    ]);
    let t = f.session.token().unwrap();
    let session = f.session.clone();
    let old = t.clone();
    let pending = tokio::spawn(async move {
        MvService::new(session)
            .unwrap()
            .download_options_for(&old, &json!("123"))
            .await
    });
    f.api.wait_for_calls(1).await;
    f.service.like_for(&t, "123", true).await.unwrap();
    send.send(Ok(detail(false))).unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::QueueChanged)));
    assert!(
        f.service
            .cached_for(&t, &json!("123"))
            .unwrap()
            .unwrap()
            .liked
    );
}
#[tokio::test]
async fn journal_failure_prevents_send_and_failed_confirmation_recovers_without_rewrite() {
    let f = Fixture::new([detail(false).into()]);
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_intent BEFORE INSERT ON state WHEN NEW.kind='mv-likes-v1' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(
        f.service
            .like_for(&f.session.token().unwrap(), "123", true)
            .await
            .is_err()
    );
    assert_eq!(f.writes(), 0);
    let f = Fixture::new([
        detail(false).into(),
        json!({"code":200}).into(),
        detail(true).into(),
        detail(true).into(),
        detail(true).into(),
        detail(true).into(),
    ]);
    let t = f.session.token().unwrap();
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_confirm BEFORE INSERT ON state WHEN NEW.kind='mv-likes-v1' AND json_extract(NEW.value,'$.phase')='confirmed' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(f.service.like_for(&t, "123", true).await.is_err());
    assert!(
        f.service
            .like_operation_for(&t, "123")
            .unwrap()
            .unwrap()
            .phase
            .pending()
    );
    db.execute_batch("DROP TRIGGER fail_confirm").unwrap();
    assert_eq!(
        f.service.reconcile_like_for(&t, "123").await.unwrap().phase,
        MutationPhase::Confirmed
    );
    assert_eq!(f.writes(), 1);
}
#[tokio::test]
async fn unavailable_flag_or_wrong_thread_never_sends_a_write() {
    for field in ["liked", "threadId"] {
        let mut input = detail(false);
        input["data"]["resource"][field] = Value::Null;
        let f = Fixture::new([input.into()]);
        assert!(
            f.service
                .like_for(&f.session.token().unwrap(), "123", true)
                .await
                .is_err()
        );
        assert_eq!(f.writes(), 0);
    }
}
#[tokio::test]
async fn supplemental_recommendations_validate_identity_and_scope_cached_failures() {
    let input = json!({"code":200,"mvs":[{"id":"90071992547409931","name":"Similar","duration":120000,"artists":[{"name":"Artist"}],"cover":"https://p1.music.126.net/a?secret=fixture"}]});
    let f = Fixture::new([input.into(), failure()]);
    let t = f.session.token().unwrap();
    let rows = f.service.recommendations_for(&t, "123").await.unwrap();
    assert_eq!(rows[0].id, "90071992547409931");
    assert_eq!(f.api.calls()[0].path, "/api/discovery/simiMV");
    assert_eq!(f.api.calls()[0].payload, json!({"mvid":"123"}));
    assert!(f.service.recommendations_for(&t, "123").await.is_err());
    assert_eq!(
        f.service
            .cached_recommendations_for(&t, "123")
            .unwrap()
            .unwrap()
            .len(),
        1
    );
    assert!(
        f.service
            .cached_recommendations_for(&t, "124")
            .unwrap()
            .is_none()
    );
    f.session
        .adopt(f.api.clone(), json!({"userId":2}), &t)
        .unwrap();
    assert!(
        f.service
            .cached_recommendations_for(&f.session.token().unwrap(), "123")
            .unwrap()
            .is_none()
    );
    for value in [
        json!({"code":200,"mvs":[{"id":4},{"id":4}]}),
        json!({"code":200,"data":[]}),
    ] {
        let f = Fixture::new([value.into()]);
        assert!(
            f.service
                .recommendations_for(&f.session.token().unwrap(), "123")
                .await
                .is_err()
        );
    }
}
