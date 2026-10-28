use cumulus_core::{
    Error,
    playback::output::{OutputPreferences, OutputSelection},
    storage::StateStore,
};
use serde_json::json;

fn device(id: &str) -> OutputSelection {
    OutputSelection::new(id.into(), "合成 USB 耳机".into()).unwrap()
}

#[test]
fn preference_is_device_local_durable_and_preserves_opaque_identifiers() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path()).unwrap();
    let initial = OutputPreferences::read(&store).unwrap();
    assert_eq!(initial.selection.id(), "auto");
    let expected =
        OutputPreferences::replace(&store, initial.revision, device("wasapi/{A-b}/device"))
            .unwrap();
    drop(store);
    let reopened = StateStore::open(dir.path()).unwrap();
    assert_eq!(OutputPreferences::read(&reopened).unwrap(), expected);
    assert!(
        reopened
            .get("account-a", "audio-output-preferences-v1", "current")
            .unwrap()
            .is_none()
    );
    assert_eq!(expected.selection.description(), "合成 USB 耳机");
}

#[test]
fn compare_and_swap_rejects_stale_windows_and_retry_has_a_new_revision() {
    let dir = tempfile::tempdir().unwrap();
    let first = StateStore::open(dir.path()).unwrap();
    let second = StateStore::open(dir.path()).unwrap();
    let saved = OutputPreferences::replace(&first, 0, device("pulse/one")).unwrap();
    assert!(OutputPreferences::replace(&second, 0, device("pulse/two")).is_err());
    let retried =
        OutputPreferences::replace(&second, saved.revision, saved.selection.clone()).unwrap();
    assert!(retried.revision > saved.revision);
    assert_eq!(retried.selection, saved.selection);
    assert_eq!(OutputPreferences::read(&first).unwrap(), retried);
}

#[test]
fn repair_is_explicit_preserves_damage_and_cannot_reuse_an_observed_revision() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path()).unwrap();
    let saved = OutputPreferences::replace(&store, 0, device("pulse/one")).unwrap();
    let raw = json!({"revision":"corrupt","selection":{"id":"pulse/one","description":false}});
    store
        .put(
            "local-device",
            "audio-output-preferences-v1",
            "current",
            &raw,
        )
        .unwrap();
    assert!(matches!(
        OutputPreferences::read(&store),
        Err(Error::Json(_))
    ));
    assert!(OutputPreferences::replace(&store, saved.revision, device("pulse/two")).is_err());
    let repaired = OutputPreferences::repair(&store).unwrap();
    assert!(repaired.revision > saved.revision);
    assert_eq!(repaired.selection.id(), "auto");
    assert!(OutputPreferences::repair(&store).is_err());
    assert!(OutputPreferences::replace(&store, saved.revision, device("pulse/two")).is_err());
    let rows = store
        .values("local-device", "audio-output-recovery-v1")
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows.values().next().unwrap(), &raw);
}

#[test]
fn malformed_native_ids_and_untrusted_extra_fields_never_become_valid_preferences() {
    for id in ["".into(), "x\0y".into(), "x".repeat(4097)] {
        assert!(OutputSelection::new(id, "label".into()).is_err());
    }
    for value in [
        json!({"id":"auto","description":"label","extra":true}),
        json!({"id":"auto","description":"x\0y"}),
        json!({"id":"auto","description":"x".repeat(16385)}),
        json!({"id":3,"description":"label"}),
    ] {
        assert!(serde_json::from_value::<OutputSelection>(value).is_err());
    }
}

#[test]
fn exhausted_or_damaged_revision_cannot_partially_replace_the_preference() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path()).unwrap();
    let saved = OutputPreferences::replace(&store, 0, device("pulse/one")).unwrap();
    for revision in [json!(u64::MAX), json!("damaged")] {
        store
            .put(
                "local-device",
                "audio-output-preferences-v1",
                "revision",
                &revision,
            )
            .unwrap();
        assert!(OutputPreferences::replace(&store, saved.revision, device("pulse/two")).is_err());
        assert_eq!(OutputPreferences::read(&store).unwrap(), saved);
    }
}

#[test]
fn missing_current_record_does_not_accept_a_stale_installation_revision() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path()).unwrap();
    let saved = OutputPreferences::replace(&store, 0, device("pulse/one")).unwrap();
    store
        .delete("local-device", "audio-output-preferences-v1", "current")
        .unwrap();
    assert!(matches!(
        OutputPreferences::read(&store),
        Err(Error::Json(_))
    ));
    assert!(OutputPreferences::replace(&store, 0, device("pulse/two")).is_err());
    let repaired = OutputPreferences::repair(&store).unwrap();
    assert!(repaired.revision > saved.revision);
    assert_eq!(repaired.selection.id(), "auto");
    assert_eq!(
        store
            .values("local-device", "audio-output-recovery-v1")
            .unwrap()
            .values()
            .next()
            .unwrap(),
        &json!({"missing":true})
    );
}
