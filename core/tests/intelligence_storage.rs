#[allow(dead_code)]
#[path = "support/intelligence_player.rs"]
mod support;
use cumulus_core::{
    Error,
    playback::{
        PlaybackMode, PlaybackQueue,
        controller::{PlaybackCommand as Cmd, PlaybackStatus},
        intelligence::IntelligenceRecovery,
    },
    storage::StateStore,
};
use serde_json::json;
use std::sync::Arc;
use support::*;

#[tokio::test]
async fn explicit_repair_preserves_the_queue_and_quarantines_original_bytes_without_replaying_audio()
 {
    let f = Fixture::new();
    act(&f, begin(7, false)).await.unwrap();
    f.command(Cmd::Pause(true)).await.unwrap();
    f.command(Cmd::Seek(14.0)).await.unwrap();
    f.command(Cmd::Stop).await.unwrap();
    let raw = f.store.get("9", "playback", "queue").unwrap();
    f.store
        .delete("9", "playback-intelligence-v1", "session")
        .unwrap();
    assert!(PlaybackQueue::new(Some(f.store.clone()), "9").is_err());
    let repair = IntelligenceRecovery::inspect(&f.store, "9")
        .unwrap()
        .unwrap();
    assert_eq!(repair.queue_len(), 1);
    let normal = repair.adopt_ordinary(&f.store).unwrap();
    assert!(normal.intelligence.is_none());
    assert_eq!(ids(&normal), vec![json!(7)]);
    let quarantine = f
        .store
        .values("9", "playback-intelligence-quarantine-v1")
        .unwrap();
    assert_eq!(quarantine.len(), 1);
    let saved = quarantine.values().next().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(saved["queue"].as_str().unwrap()).unwrap(),
        raw.unwrap()
    );
    let now = f.command(Cmd::Snapshot).await.unwrap();
    assert!(now.intelligence.is_none());
    assert_eq!(now.state, PlaybackStatus::Idle);
    assert!(now.play_id.is_none());
    assert!(
        IntelligenceRecovery::inspect(&f.store, "10")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn repair_is_atomic_and_a_stale_ticket_cannot_overwrite_newer_ordinary_work() {
    let f = Fixture::new();
    act(&f, begin(7, false)).await.unwrap();
    let repair = IntelligenceRecovery::inspect(&f.store, "9")
        .unwrap()
        .unwrap();
    f.command(replace(&[70, 71])).await.unwrap();
    assert!(matches!(
        repair.adopt_ordinary(&f.store),
        Err(Error::QueueChanged)
    ));
    assert!(
        f.store
            .values("9", "playback-intelligence-quarantine-v1")
            .unwrap()
            .is_empty()
    );
    act(&f, begin(7, true)).await.unwrap();
    let raw = f.store.get("9", "playback", "queue").unwrap();
    let session = f
        .store
        .get("9", "playback-intelligence-v1", "session")
        .unwrap();
    f.sql("CREATE TRIGGER reject_repair BEFORE UPDATE ON state WHEN NEW.kind='playback' AND NEW.key='queue' BEGIN SELECT RAISE(ABORT,'synthetic repair failure'); END;");
    let repair = IntelligenceRecovery::inspect(&f.store, "9")
        .unwrap()
        .unwrap();
    assert!(repair.adopt_ordinary(&f.store).is_err());
    assert_eq!(f.store.get("9", "playback", "queue").unwrap(), raw);
    assert_eq!(
        f.store
            .get("9", "playback-intelligence-v1", "session")
            .unwrap(),
        session
    );
    assert!(
        f.store
            .values("9", "playback-intelligence-quarantine-v1")
            .unwrap()
            .is_empty()
    );
    f.sql("DROP TRIGGER reject_repair");
}

#[tokio::test]
async fn malformed_orphan_state_does_not_revive_ai_or_block_an_ordinary_queue() {
    let f = Fixture::new();
    f.command(replace(&[70, 71])).await.unwrap();
    f.sql("INSERT INTO state(owner,kind,key,value) VALUES('9','playback-intelligence-v1','session','{broken');");
    let opened = PlaybackQueue::new(Some(f.store.clone()), "9").unwrap();
    assert!(opened.intelligence_context().is_none());
    assert_eq!(ids(&opened.snapshot()), vec![json!(70), json!(71)]);
    let ordinary = f
        .command(Cmd::Mode(PlaybackMode::Sequential))
        .await
        .unwrap();
    assert!(ordinary.intelligence.is_none());
    assert!(
        f.store
            .get("9", "playback-intelligence-v1", "session")
            .unwrap()
            .is_none()
    );
    f.store
        .put(
            "9",
            "playback-intelligence-v1",
            "session",
            &json!({"broken":true}),
        )
        .unwrap();
    f.store
        .put(
            "9",
            "playback",
            "queue",
            &json!({"items":[],"order":["ghost"]}),
        )
        .unwrap();
    assert!(IntelligenceRecovery::inspect(&f.store, "9").is_err());
    assert_eq!(
        f.store.get("9", "playback", "queue").unwrap(),
        Some(json!({"items":[],"order":["ghost"]}))
    );
}

#[tokio::test]
async fn ordinary_selection_does_not_rewrite_immutable_ai_backups() {
    let f = Fixture::new();
    let id = act(&f, begin(7, false))
        .await
        .unwrap()
        .intelligence
        .unwrap()
        .id;
    act(&f, complete(&id, &[90, 91])).await.unwrap();
    let before = f
        .store
        .get("9", "playback-intelligence-v1", "session")
        .unwrap();
    f.sql("CREATE TRIGGER reject_backup_rewrite BEFORE INSERT ON state WHEN NEW.kind='playback-intelligence-v1' BEGIN SELECT RAISE(ABORT,'backup should not be rewritten'); END;");
    assert_eq!(song(&f.command(Cmd::Next).await.unwrap()), Some(90));
    assert_eq!(song(&f.command(Cmd::Previous).await.unwrap()), Some(7));
    assert_eq!(
        f.store
            .get("9", "playback-intelligence-v1", "session")
            .unwrap(),
        before
    );
    f.sql("DROP TRIGGER reject_backup_rewrite");
}

#[test]
fn queue_change_stamp_observes_both_connections_local_writes_and_rollback_without_false_queue_edits()
 {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let mut queue = PlaybackQueue::new(Some(store.clone()), "9").unwrap();
    queue.replace(vec![json!({"id":7})], 0, json!({})).unwrap();
    assert!(!queue.reload().unwrap());
    let other = Arc::new(StateStore::open(root.path()).unwrap());
    let mut second = PlaybackQueue::new(Some(other), "9").unwrap();
    second.replace(vec![json!({"id":8})], 0, json!({})).unwrap();
    assert!(queue.reload().unwrap());
    assert_eq!(ids(&queue.snapshot()), vec![json!(8)]);
    let mut raw = store.get("9", "playback", "queue").unwrap().unwrap();
    raw["items"][0]["resource"]["id"] = json!(9);
    store.put("9", "playback", "queue", &raw).unwrap();
    assert!(queue.reload().unwrap());
    assert_eq!(ids(&queue.snapshot()), vec![json!(9)]);
    let revision = queue.revision();
    store.put("9", "unrelated", "cache", &json!(1)).unwrap();
    assert!(!queue.reload().unwrap());
    assert_eq!(queue.revision(), revision);
    let result: cumulus_core::Result<()> = store.transaction(|tx| {
        tx.put("9", "playback", "queue", &json!({}))?;
        Err(Error::State("synthetic rollback"))
    });
    assert!(result.is_err());
    assert!(!queue.reload().unwrap());
    assert_eq!(ids(&queue.snapshot()), vec![json!(9)]);
}
