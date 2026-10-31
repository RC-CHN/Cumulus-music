use cumulus_core::{
    fm::FmSource,
    playback::{PlaybackMode, PlaybackQueue, QueueSnapshot},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::Arc;

fn empty() -> Value {
    serde_json::to_value(QueueSnapshot {
        mode: PlaybackMode::SingleLoop,
        fm_source: Some(FmSource::Personal),
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn empty_fm_reopens_and_replacing_it_with_an_empty_normal_queue_releases_backup_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(dir.path()).unwrap());
    store.put("1", "playback", "queue", &empty()).unwrap();
    store
        .put("1", "playback", "fm-backup-queue", &json!({}))
        .unwrap();
    store
        .put("1", "fm-player-v1", "source", &json!({"kind":"personal"}))
        .unwrap();
    let mut queue = PlaybackQueue::new(Some(store.clone()), "1").unwrap();
    assert!(queue.is_fm());
    assert!(queue.is_empty());
    assert!(
        !PlaybackQueue::new(Some(store.clone()), "2")
            .unwrap()
            .is_fm()
    );
    queue.replace(vec![], 0, Value::Null).unwrap();
    assert!(!queue.is_fm());
    assert!(
        store
            .get("1", "playback", "fm-backup-queue")
            .unwrap()
            .is_none()
    );
    assert!(store.get("1", "fm-player-v1", "source").unwrap().is_none());
    let reopened = PlaybackQueue::new(Some(store.clone()), "1").unwrap();
    assert!(!reopened.is_fm());
    assert_eq!(reopened.snapshot(), queue.snapshot());
    assert!(
        store
            .get("1", "playback", "queue")
            .unwrap()
            .unwrap()
            .get("fm_source")
            .is_none()
    );
}

#[test]
fn legacy_empty_queue_uses_only_its_own_valid_source_and_backup_without_writing_on_read() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(dir.path()).unwrap());
    let legacy = json!({"items":[],"order":[],"current":null,"mode":"sequential"});
    store.put("1", "playback", "queue", &legacy).unwrap();
    store
        .put("1", "fm-player-v1", "source", &json!({"kind":"personal"}))
        .unwrap();
    assert!(
        !PlaybackQueue::new(Some(store.clone()), "1")
            .unwrap()
            .is_fm()
    );
    store
        .put("1", "playback", "fm-backup-queue", &json!({}))
        .unwrap();
    let mut queue = PlaybackQueue::new(Some(store.clone()), "1").unwrap();
    assert!(queue.is_fm());
    assert_eq!(store.get("1", "playback", "queue").unwrap(), Some(legacy));
    queue.set_mode(PlaybackMode::SingleLoop).unwrap();
    assert_eq!(store.get("1", "playback", "queue").unwrap(), Some(empty()));
    assert!(
        PlaybackQueue::new(Some(store.clone()), "1")
            .unwrap()
            .is_fm()
    );
    let normal = json!({"items":[],"order":[],"current":null,"mode":"random"});
    store.put("1", "playback", "queue", &normal).unwrap();
    assert!(queue.reload().unwrap());
    assert!(!queue.is_fm());
}

#[test]
fn inconsistent_fm_identity_is_rejected_without_repairing_or_erasing_user_state() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(dir.path()).unwrap());
    let mut wrong_mode = empty();
    wrong_mode["mode"] = json!("random");
    let mut wrong_source = empty();
    wrong_source["items"] =
        json!([{"entry_id":"a","resource":{"id":1},"source":{"kind":"playlist"}}]);
    wrong_source["order"] = json!(["a"]);
    let mut invalid_source = empty();
    invalid_source["fm_source"] = json!({"kind":"zone","rule_id":""});
    for value in [wrong_mode, wrong_source, invalid_source] {
        store.put("1", "playback", "queue", &value).unwrap();
        assert!(PlaybackQueue::new(Some(store.clone()), "1").is_err());
        assert_eq!(store.get("1", "playback", "queue").unwrap(), Some(value));
    }
}
