use cumulus_core::{Error, relay::*};
use serde_json::{Value, json};
fn resource(id: &str, kind: RelayResourceKind) -> RelayResource {
    RelayResource::new(&json!(id), kind).unwrap()
}
fn state() -> RelayMachine {
    RelayMachine::start(
        RelaySource::Resources {
            resources: vec![resource("1", RelayResourceKind::Song)],
        },
        resource("1", RelayResourceKind::Song),
    )
    .unwrap()
}
fn value(upload: RelayUpload) -> Value {
    match upload {
        RelayUpload::Source { value, .. }
        | RelayUpload::State(value)
        | RelayUpload::Commands(value) => value,
    }
}

#[test]
fn cumulative_commands_survive_roundtrip_and_bulk_voice_delete_is_not_truncated() {
    let mut state = state();
    let voices = vec![
        resource("2", RelayResourceKind::Voice),
        resource("3", RelayResourceKind::Voice),
    ];
    state.insert(voices.clone()).unwrap();
    let uploaded = value(state.delete(voices.clone()).unwrap());
    assert_eq!(
        uploaded["commands"],
        json!([{"command":"insert","resources":voices,"preId":"1"},{"command":"delete","resources":voices}])
    );
    let mut reloaded: RelayMachine =
        serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
    assert_eq!(value(reloaded.commands_upload().unwrap()), uploaded);
    assert_eq!(
        value(reloaded.clear().unwrap())["commands"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let before = serde_json::to_value(&reloaded).unwrap();
    assert!(
        reloaded
            .command(RelayCommand::Move {
                resources: vec![],
                pre_id: None
            })
            .is_err()
    );
    assert_eq!(serde_json::to_value(&reloaded).unwrap(), before);
}
#[test]
fn snapshot_resets_command_history_uses_entry_identity_and_has_bounded_recovery() {
    let mut state = state();
    state
        .delete(vec![resource("4", RelayResourceKind::Song)])
        .unwrap();
    let entries = (0..200)
        .map(|i| SnapshotItem {
            entry_id: format!("entry-{i}"),
            resource: resource(&(i + 1000).to_string(), RelayResourceKind::Song),
        })
        .collect::<Vec<_>>();
    let upload = state
        .force_snapshot(SnapshotFrame {
            items: &entries,
            selected_entry: Some("entry-100"),
            current: resource("1100", RelayResourceKind::Song),
            config: &RelayConfig::default(),
            mode: Some(RelayMode::Random),
        })
        .unwrap();
    assert_eq!(
        value(upload.clone())["resources"].as_array().unwrap().len(),
        100
    );
    assert_eq!(value(upload.clone())["resources"][0]["id"], "1050");
    assert_eq!(state.session_id().len(), 12);
    assert_eq!(
        value(state.commands_upload().unwrap())["commands"],
        json!([])
    );
    assert!(!upload.requires_snapshot(&Error::service(
        SOURCE_PATH,
        json!(10001),
        json!({"code":10001})
    )));
    assert!(
        state
            .select(resource("5000", RelayResourceKind::Voice), None)
            .unwrap()
            .1
    );
    let before = serde_json::to_value(&state).unwrap();
    assert!(
        state
            .force_snapshot(SnapshotFrame {
                items: &[],
                selected_entry: None,
                current: resource("1", RelayResourceKind::Song),
                config: &RelayConfig::default(),
                mode: None
            })
            .is_err()
    );
    assert_eq!(serde_json::to_value(&state).unwrap(), before);
    assert_eq!(snapshot_window(0, 0, 100), 0..0);
    assert_eq!(snapshot_window(200, 199, 101), 149..200);
}
#[test]
fn mixed_pull_preserves_type_and_same_numeric_id_does_not_hide_current_voice() {
    let plan = PullPlan::from_data(&json!({"relayMode":"snapshot","resources":[{"id":"1","type":"song"},{"id":"2","type":"voice"}],"currentPlayState":{"resourceId":"1","resourceType":"voice","progress":12500},"playMode":"unrecognized"})).unwrap();
    assert_eq!(plan.position_ms, 12500);
    assert_eq!(plan.mode, None);
    let PullSelection::Resources { resources } = plan.selection else {
        panic!("expected snapshot");
    };
    assert_eq!(
        resources,
        vec![
            resource("1", RelayResourceKind::Voice),
            resource("1", RelayResourceKind::Song),
            resource("2", RelayResourceKind::Voice)
        ]
    );
    assert!(
        PullPlan::from_data(
            &json!({"relayMode":"snapshot","resources":[{"id":"1","type":"video"}]})
        )
        .is_err()
    );
}
#[test]
fn voice_clock_matches_leading_throttle_distinct_and_reset_without_song_progress_uploads() {
    let mut clock = VoiceProgressClock::default();
    assert_eq!(clock.sample(0, 20, 0, false, false), None);
    assert_eq!(clock.sample(1000, 20, 1000, true, true), None);
    assert_eq!(clock.sample(20_000, 20, 5000, true, true), Some(5000));
    assert_eq!(clock.sample(40_000, 20, 5000, true, true), None);
    assert_eq!(clock.sample(41_000, 20, 6000, true, true), None);
    assert_eq!(clock.sample(60_000, 20, 6000, true, false), None);
    assert_eq!(clock.sample(80_000, 20, 6000, true, true), None);
    clock.reset();
    assert_eq!(clock.sample(80_001, 20, 6000, true, true), Some(6000));
    let mut state = state();
    assert!(state.voice_progress(6000, None).is_err());
    state
        .select(resource("3", RelayResourceKind::Voice), None)
        .unwrap();
    assert_eq!(
        value(state.voice_progress(6000, None).unwrap())["progress"],
        6000
    );
    assert!(
        state
            .mode_change(RelayMode::Random, true)
            .unwrap()
            .is_none()
    );
}
#[test]
fn invalid_server_configuration_cannot_enable_unbounded_work_and_local_sources_are_excluded() {
    assert_eq!(
        RelayConfig::from_data(&json!({"enable":true,"radioProgressInterval":1,"snapshotSize":10}))
            .unwrap(),
        RelayConfig {
            enable: true,
            radio_progress_interval: 3,
            snapshot_size: 100
        }
    );
    for bad in [
        json!({"enable":"true"}),
        json!({"radioProgressInterval":-1}),
        json!({"snapshotSize":100.5}),
        json!({"snapshotSize":50_001}),
        Value::Null,
    ] {
        assert!(RelayConfig::from_data(&bad).is_err());
    }
    for scene in ["cloud", "local", "download", "fm", "zoneFm", "aiMode"] {
        assert!(RelaySource::excluded_scene(scene));
    }
    assert!(!RelaySource::excluded_scene("playlistHandoff"));
    assert!(RelayAlert::from_official(json!({"deviceId":""})).is_err());
}
