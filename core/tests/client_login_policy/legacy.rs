use super::*;
use cumulus_core::playback::{PlaybackMode, PlaybackQueue};

fn store(w: &World) -> StateStore {
    StateStore::open(w.root.path().join("host")).unwrap()
}
fn queue(w: &World, owner: &str, id: u64) -> Value {
    let store = Arc::new(store(w));
    let mut queue = PlaybackQueue::new(Some(store.clone()), owner).unwrap();
    queue
        .replace(vec![json!({"id":id})], 0, json!({"kind":"playlist"}))
        .unwrap();
    store
        .put(
            owner,
            "playback",
            "position",
            &json!({"entry_id":queue.snapshot().current,"seconds":12}),
        )
        .unwrap();
    serde_json::to_value(queue.snapshot()).unwrap()
}
fn force_exit() -> HttpResponse {
    json!({"code":200,"data":{"action":2,"followAction":1,"title":"重新登录","message":"请确认当前账号"}}).into()
}

#[tokio::test]
async fn original_pc_string_301_normalization_reaches_both_interceptors_and_expiry_consumers() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/login_legacy.json")).unwrap();
    for case in fixture["bridge"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["networkEvents"] == json!(["301"]))
    {
        for group in ["c", "t1"] {
            let code = case["steps"][0]["input"]["response"]["code"].clone();
            let w = World::new([
                json!({"code":code}).into(),
                flags(group, group),
                force_exit(),
            ])
            .await;
            assert!(
                w.client
                    .daily_songs()
                    .await
                    .err()
                    .unwrap()
                    .is_service_code(301)
            );
            assert_eq!(w.client.account().unwrap().status, SessionStatus::Expired);
            assert_eq!(w.http.calls().len(), 4);
        }
    }
}
#[tokio::test(start_paused = true)]
async fn legacy_valid_cooldown_uses_plain_pc_request_and_never_replays_the_business_call() {
    let w = World::new([
        rejected(),
        flags("c", "c"),
        validation(json!(1)),
        rejected(),
        rejected(),
        validation(json!(1)),
    ])
    .await;
    let epoch = w.epoch();
    assert!(matches!(
        w.client.daily_songs().await,
        Err(Error::LoginDeferred)
    ));
    tokio::time::advance(std::time::Duration::from_millis(4999)).await;
    assert!(matches!(
        w.client.daily_songs().await,
        Err(Error::LoginDeferred)
    ));
    tokio::time::advance(std::time::Duration::from_millis(1)).await;
    assert!(matches!(
        w.client.daily_songs().await,
        Err(Error::LoginDeferred)
    ));
    assert_eq!(w.epoch(), epoch);
    assert_eq!(
        w.client.account().unwrap().status,
        SessionStatus::Authenticated
    );
    let calls = w.http.calls();
    assert_eq!(calls.len(), 7);
    for call in calls
        .iter()
        .filter(|c| c.path == "/api/middle/account/token/refresh")
    {
        assert_eq!(call.payload.get("fromPage"), None);
        assert_eq!(call.payload.get("fromAPI"), None);
        assert_eq!(call.login_chain, None);
    }
}
#[tokio::test]
async fn legacy_only_three_null_actions_expire_while_missing_actions_reset_and_keep_the_queue() {
    let mut responses = vec![rejected(), flags("c", "c"), validation(Value::Null)];
    responses.extend([rejected(), json!({"code":200,"data":{}}).into()]);
    for _ in 0..3 {
        responses.extend([rejected(), validation(Value::Null)]);
    }
    let w = World::new(responses).await;
    let before = queue(&w, "1", 10);
    for _ in 0..4 {
        assert!(matches!(
            w.client.daily_songs().await,
            Err(Error::LoginDeferred)
        ));
    }
    assert!(
        w.client
            .daily_songs()
            .await
            .err()
            .unwrap()
            .is_service_code(301)
    );
    assert_eq!(w.client.account().unwrap().status, SessionStatus::Expired);
    assert_eq!(
        store(&w).get("1", "playback", "queue").unwrap(),
        Some(before)
    );
    assert_eq!(w.record()["queue_reset"], Value::Null);
    assert_eq!(
        w.client.account().unwrap().login_notice.unwrap().message,
        "请确认当前账号"
    );
}
#[tokio::test]
async fn real_legacy_factory_code_gate_matches_original_bridge_without_reproducing_the_deadlock() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/login_legacy.json")).unwrap();
    for case in fixture["bridge"].as_array().unwrap() {
        let response = case["steps"][0]["input"]["response"].clone();
        let w = World::new([
            rejected(),
            flags("c", "c"),
            response.into(),
            rejected(),
            validation(json!(1)),
        ])
        .await;
        assert!(
            matches!(w.client.daily_songs().await, Err(Error::LoginDeferred)),
            "{}",
            case["name"]
        );
        assert!(matches!(
            w.client.daily_songs().await,
            Err(Error::LoginDeferred)
        ));
        assert_eq!(
            w.client.account().unwrap().status,
            SessionStatus::Authenticated
        );
        // Original rejection latches its subscriber forever. Rust retries on
        // a subsequent business failure; a valid first response keeps cooldown.
        let first_valid = case["steps"][0]["expected"]["kind"] == "valid";
        assert_eq!(
            w.http.calls().len(),
            if first_valid { 5 } else { 6 },
            "{}",
            case["name"]
        );
    }
}
#[tokio::test]
async fn legacy_transport_failure_is_recoverable_and_does_not_consume_business_failure_count() {
    let w = World::new([
        rejected(),
        flags("c", "c"),
        HttpResponse::Ready(Err(Error::Transport("synthetic".into()))),
        rejected(),
        validation(Value::Null),
        rejected(),
        validation(Value::Null),
        rejected(),
        validation(Value::Null),
    ])
    .await;
    for _ in 0..3 {
        assert!(matches!(
            w.client.daily_songs().await,
            Err(Error::LoginDeferred)
        ));
    }
    assert!(
        w.client
            .daily_songs()
            .await
            .err()
            .unwrap()
            .is_service_code(301)
    );
    assert_eq!(w.client.account().unwrap().status, SessionStatus::Expired);
}
#[tokio::test]
async fn server_clear_removes_only_current_account_queue_and_fm_resume_state_preserving_mode() {
    let w = World::new([rejected(), flags("c", "c"), force_exit()]).await;
    let other = queue(&w, "2", 20);
    let guest = queue(&w, "local-guest", 30);
    let s = Arc::new(store(&w));
    let mut q = PlaybackQueue::new(Some(s.clone()), "1").unwrap();
    q.replace(vec![json!({"id":10})], 0, json!({"kind":"fm"}))
        .unwrap();
    s.put(
        "1",
        "playback",
        "fm-backup-queue",
        &json!({"items":[],"order":[],"current":null,"mode":"random"}),
    )
    .unwrap();
    for (kind, key) in [
        ("playback", "position"),
        ("playback", "fm-backup-position"),
        ("fm-player-v1", "source"),
        ("fm-player-v1", "failure"),
    ] {
        s.put("1", kind, key, &json!({})).unwrap();
    }
    s.put("1", "test-resource", "keep", &json!(true)).unwrap();
    assert!(
        w.client
            .daily_songs()
            .await
            .err()
            .unwrap()
            .is_service_code(301)
    );
    let cleared = PlaybackQueue::new(Some(s.clone()), "1").unwrap();
    assert!(cleared.is_empty());
    assert_eq!(cleared.mode(), PlaybackMode::Random);
    for (kind, key) in [
        ("playback", "position"),
        ("playback", "fm-backup-queue"),
        ("playback", "fm-backup-position"),
        ("fm-player-v1", "source"),
        ("fm-player-v1", "failure"),
    ] {
        assert!(s.get("1", kind, key).unwrap().is_none());
    }
    assert_eq!(s.get("2", "playback", "queue").unwrap(), Some(other));
    assert_eq!(
        s.get("local-guest", "playback", "queue").unwrap(),
        Some(guest)
    );
    assert_eq!(
        s.get("1", "test-resource", "keep").unwrap(),
        Some(json!(true))
    );
    assert_eq!(w.record()["queue_reset"], Value::Null);
    assert_eq!(
        s.values("1", "login-queue-reset-v1")
            .unwrap()
            .values()
            .collect::<Vec<_>>(),
        vec![&json!("cleared")]
    );
    assert!(!w.client.account().unwrap().can_sign_out);
}
#[tokio::test]
async fn failed_queue_transaction_keeps_durable_intent_and_restart_finishes_without_http() {
    for supersede in [false, true] {
        let w = World::new([rejected(), flags("c", "c"), force_exit()]).await;
        let before = queue(&w, "1", 10);
        let db = rusqlite::Connection::open(w.root.path().join("host/core.sqlite3")).unwrap();
        db.execute_batch("CREATE TRIGGER reject_cleanup BEFORE DELETE ON state WHEN OLD.owner='1' AND OLD.kind='playback' AND OLD.key='position' BEGIN SELECT RAISE(ABORT,'synthetic disk failure'); END;").unwrap();
        assert!(matches!(
            w.client.daily_songs().await,
            Err(Error::Storage(_))
        ));
        assert_eq!(w.client.account().unwrap().status, SessionStatus::Expired);
        assert!(w.client.account().unwrap().can_sign_out);
        assert!(w.credentials().login_credentials().unwrap().token.is_none());
        assert!(w.record()["queue_reset"].is_object());
        assert_eq!(
            store(&w).get("1", "playback", "queue").unwrap(),
            Some(before)
        );
        let new = if supersede {
            Some(queue(&w, "1", 11))
        } else {
            None
        };
        db.execute_batch("DROP TRIGGER reject_cleanup").unwrap();
        let http = Arc::new(ScriptHttp::new([]));
        let reopened = World::open(w.root.path(), http.clone());
        assert_eq!(
            reopened.restore_account().await.unwrap().status,
            SessionStatus::Expired
        );
        assert!(http.calls().is_empty());
        assert_eq!(w.record()["queue_reset"], Value::Null);
        let value = store(&w).get("1", "playback", "queue").unwrap().unwrap();
        if let Some(new) = new {
            assert_eq!(value, new);
        } else {
            assert_eq!(value["items"], json!([]));
        }
        assert_eq!(
            store(&w)
                .values("1", "login-queue-reset-v1")
                .unwrap()
                .values()
                .collect::<Vec<_>>(),
            vec![&json!(if supersede { "superseded" } else { "cleared" })]
        );
    }
}
#[tokio::test]
async fn failed_ack_replays_receipt_without_clearing_a_later_queue_and_local_logout_recovers() {
    let w = World::new([rejected(), flags("c", "c"), force_exit()]).await;
    queue(&w, "1", 10);
    let db = rusqlite::Connection::open(w.root.path().join("profile/core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_ack BEFORE INSERT ON state WHEN NEW.kind='credentials' AND json_type(NEW.value,'$.queue_reset') IS NULL AND EXISTS(SELECT 1 FROM state WHERE kind='credentials' AND json_type(value,'$.queue_reset')='object') BEGIN SELECT RAISE(ABORT,'synthetic ack failure'); END;").unwrap();
    assert!(matches!(
        w.client.daily_songs().await,
        Err(Error::Storage(_))
    ));
    assert_eq!(
        store(&w).get("1", "playback", "queue").unwrap().unwrap()["items"],
        json!([])
    );
    let new = queue(&w, "1", 11);
    db.execute_batch("DROP TRIGGER reject_ack").unwrap();
    let logout = w.client.logout_account(&w.epoch(), false).await.unwrap();
    assert_eq!(logout.account.status, SessionStatus::SignedOut);
    assert_eq!(w.record()["queue_reset"], Value::Null);
    assert_eq!(store(&w).get("1", "playback", "queue").unwrap(), Some(new));
    assert_eq!(w.http.calls().len(), 4);
}
#[tokio::test]
async fn late_legacy_exit_cannot_clear_queues_or_replace_a_new_account_notice() {
    let (pending, sender) = gate();
    let w = World::new([rejected(), flags("c", "c"), pending]).await;
    let before = queue(&w, "1", 10);
    let client = w.client.clone();
    let task = tokio::spawn(async move { client.daily_songs().await });
    w.http.wait(4).await;
    w.client.logout_account(&w.epoch(), false).await.unwrap();
    let HttpResponse::Ready(response) = force_exit() else {
        unreachable!()
    };
    assert!(sender.send(response).is_ok());
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(w.client.account().unwrap().status, SessionStatus::SignedOut);
    assert!(w.client.account().unwrap().login_notice.is_none());
    assert_eq!(
        store(&w).get("1", "playback", "queue").unwrap(),
        Some(before)
    );
}
