use cumulus_core::{
    Error,
    playback::{PlaybackMode, PlaybackQueue, Step, Timeline, TimelineMode},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};

#[test]
fn queue_matches_frozen_recovered_transitions_including_random_display_reorder() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/legacy-queue.json")).unwrap();
    for trace in fixture["traces"].as_array().unwrap() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(tmp.path()).unwrap());
        store
            .put("1", "playback", "queue", &trace["initial"])
            .unwrap();
        let mut queue = PlaybackQueue::new(Some(store.clone()), "1").unwrap();
        for step in trace["steps"].as_array().unwrap() {
            let args = &step["args"];
            match step["method"].as_str().unwrap() {
                "advance" => {
                    queue
                        .advance(
                            if args[0] == json!(-1) {
                                Step::Previous
                            } else {
                                Step::Next
                            },
                            step["kwargs"]["ended"].as_bool().unwrap_or(false),
                        )
                        .unwrap();
                }
                "reorder" => queue
                    .reorder(&serde_json::from_value::<Vec<String>>(args[0].clone()).unwrap())
                    .unwrap(),
                "select" => {
                    queue.select(args[0].as_str().unwrap()).unwrap();
                }
                "remove" => queue
                    .remove(&serde_json::from_value::<Vec<String>>(args[0].clone()).unwrap())
                    .unwrap(),
                "set_mode" => queue
                    .set_mode(serde_json::from_value(args[0].clone()).unwrap())
                    .unwrap(),
                _ => panic!("unmapped transition"),
            }
            assert_eq!(
                serde_json::to_value(queue.snapshot()).unwrap(),
                step["state"],
                "{}",
                step["method"]
            );
            assert_eq!(
                store.get("1", "playback", "queue").unwrap(),
                Some(step["state"].clone())
            );
        }
    }
}

#[test]
fn duplicate_songs_have_stable_distinct_entries_and_shuffle_survives_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let mut queue = PlaybackQueue::new(Some(store.clone()), "1").unwrap();
    queue
        .replace(
            vec![json!({"id":1}), json!({"id":1}), json!({"id":2})],
            1,
            Value::Null,
        )
        .unwrap();
    let ids = queue.snapshot().order;
    assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 3);
    assert_eq!(queue.selected().unwrap().entry_id, ids[1]);
    queue.set_mode(PlaybackMode::Random).unwrap();
    let shuffled = queue.snapshot();
    let restored = PlaybackQueue::new(Some(store.clone()), "1").unwrap();
    assert_eq!(restored.snapshot(), shuffled);
    assert!(
        PlaybackQueue::new(Some(store), "2")
            .unwrap()
            .snapshot()
            .items
            .is_empty()
    );
    let added = queue
        .append(
            vec![json!({"id":1}), json!({"id":3})],
            json!({"type":"search"}),
        )
        .unwrap();
    assert_eq!(added.len(), 2);
    assert_eq!(queue.snapshot().order[..3], shuffled.order);
    assert_eq!(queue.snapshot().items[3].source, json!({"type":"search"}));
}

#[test]
fn competing_host_change_is_loaded_and_failed_edit_never_overwrites_it() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let mut first = PlaybackQueue::new(Some(store), "1").unwrap();
    first
        .replace(vec![json!({"id":1}), json!({"id":2})], 0, Value::Null)
        .unwrap();
    let other = Arc::new(StateStore::open(tmp.path()).unwrap());
    let mut second = PlaybackQueue::new(Some(other), "1").unwrap();
    let removed = first.snapshot().order[0].clone();
    first.remove(std::slice::from_ref(&removed)).unwrap();
    assert!(matches!(
        second.update_resource(&removed, json!({"path":"moved"})),
        Err(Error::QueueChanged)
    ));
    assert_eq!(second.snapshot(), first.snapshot());
    assert_eq!(second.revision(), 1);
    assert!(!second.reload().unwrap());
    let survivor = second.selected().unwrap().entry_id;
    second
        .update_resource(&survivor, json!({"path":"moved.flac"}))
        .unwrap();
    assert!(first.reload().unwrap());
    assert_eq!(
        first.selected().unwrap().resource,
        json!({"path":"moved.flac"})
    );
}

#[test]
fn persistence_failure_keeps_the_last_committed_selection_in_memory() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let mut queue = PlaybackQueue::new(Some(store), "1").unwrap();
    queue
        .replace(vec![json!({"id":1}), json!({"id":2})], 0, Value::Null)
        .unwrap();
    let before = queue.snapshot();
    rusqlite::Connection::open(tmp.path().join("core.sqlite3"))
        .unwrap()
        .execute_batch("DROP TABLE state")
        .unwrap();
    assert!(matches!(
        queue.select(&before.order[1]),
        Err(Error::Storage(_))
    ));
    assert_eq!(queue.snapshot(), before);
    assert_eq!(queue.revision(), 2);
}

#[test]
fn invalid_edits_leave_queue_intact_and_sequential_end_does_not_advance() {
    let mut queue = PlaybackQueue::new(None, "local").unwrap();
    queue
        .replace(vec![json!(1), json!(2)], 0, Value::Null)
        .unwrap();
    let before = queue.snapshot();
    assert!(
        queue
            .reorder(&[before.order[0].clone(), before.order[0].clone()])
            .is_err()
    );
    assert!(queue.remove(&["missing".into()]).is_err());
    assert!(queue.replace(vec![json!(3)], 1, Value::Null).is_err());
    assert_eq!(queue.snapshot(), before);
    queue.set_mode(PlaybackMode::Sequential).unwrap();
    assert!(queue.peek(Step::Previous, false).is_none());
    queue.select(&before.order[1]).unwrap();
    let revision = queue.revision();
    assert!(queue.advance(Step::Next, true).unwrap().is_none());
    assert_eq!(queue.revision(), revision);
    queue.remove(&before.order).unwrap();
    assert!(queue.selected().is_none());
}

#[test]
fn corrupted_persisted_queue_is_rejected_instead_of_silently_reset() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    for value in [
        json!({"items":[],"order":["ghost"]}),
        json!({"items":[],"current":"ghost"}),
        json!({"mode":"invented"}),
    ] {
        store.put("1", "playback", "queue", &value).unwrap();
        assert!(PlaybackQueue::new(Some(store.clone()), "1").is_err());
        assert_eq!(store.get("1", "playback", "queue").unwrap(), Some(value));
    }
}

#[test]
fn timeline_seek_agrees_with_frozen_official_module_110_callbacks() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/official-timeline.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let input = &case["input"];
        let expected = &case["output"];
        let timeline =
            Timeline::from_source(&json!({"rights":{"freeTrialInfo":input["trial"]}}), None)
                .unwrap();
        let position = input["position"].as_f64().unwrap();
        if let Some(dispatch) = expected["dispatch"].as_array().unwrap().first() {
            assert_eq!(
                timeline.to_media(position).unwrap(),
                dispatch["payload"]["duration"].as_f64().unwrap()
            );
        } else {
            assert!(timeline.to_media(position).is_err());
            assert_eq!(
                timeline
                    .from_media(input["current"].as_f64().unwrap())
                    .unwrap(),
                expected["restored"][0].as_f64().unwrap()
            );
        }
    }
}

#[test]
fn cue_and_preview_have_distinct_clocks_and_cannot_escape_authorized_bounds() {
    let cue =
        Timeline::from_source(&json!({"segment":{"start":40,"duration":20}}), Some(100.0)).unwrap();
    assert_eq!(cue.to_media(5.0).unwrap(), 45.0);
    assert_eq!(cue.from_media(45.0).unwrap(), 5.0);
    assert_eq!(cue.media_end(), Some(60.0));
    assert_eq!(cue.from_media(90.0).unwrap(), 20.0);
    let trial = Timeline::from_source(
        &json!({"rights":{"freeTrialInfo":{"start":40,"end":70}},"track_duration":180}),
        Some(20.0),
    )
    .unwrap();
    assert_eq!(trial.to_media(45.0).unwrap(), 5.0);
    assert_eq!(trial.from_media(5.0).unwrap(), 45.0);
    assert_eq!(trial.display_end(), Some(60.0));
    assert_eq!(trial.snapshot().total, Some(180.0));
    for position in [39.0, 61.0, f64::NAN, f64::INFINITY] {
        assert!(trial.to_media(position).is_err());
    }
    assert_eq!(
        Timeline::from_source(&json!({"full_trial":true}), Some(180.0))
            .unwrap()
            .snapshot()
            .mode,
        TimelineMode::FullTrial
    );
}

#[test]
fn timeline_rejects_malformed_metadata_and_incompatible_segments() {
    for trial in [
        json!({}),
        json!([]),
        json!({"start":true,"end":4}),
        json!({"start":-1,"end":4}),
        json!({"start":4,"end":3}),
    ] {
        assert!(Timeline::from_source(&json!({"rights":{"freeTrialInfo":trial}}), None).is_err());
    }
    assert!(Timeline::from_source(&json!({}), Some(f64::NAN)).is_err());
    assert!(
        Timeline::from_source(&json!({"segment":{"start":90,"duration":11}}), Some(100.0)).is_err()
    );
    assert!(
        Timeline::from_source(
            &json!({"rights":{"freeTrialInfo":{"start":40,"end":70}},"track_duration":50}),
            None
        )
        .is_err()
    );
    assert!(Timeline::from_source(&json!({"segment":{"start":0,"duration":5},"rights":{"freeTrialInfo":{"start":0,"end":5}}}),None).is_err());
}
