use cumulus_core::{
    preferences::shortcuts::{Action, Chord, Scope, ShortcutSettings as Settings},
    storage::StateStore,
};
use serde_json::json;
const OWNER: &str = "local-device";
const KIND: &str = "shortcut-settings-v1";
#[test]
fn malformed_json_is_preserved_as_text_by_atomic_repair() {
    let root = tempfile::tempdir().unwrap();
    let store = StateStore::open(root.path()).unwrap();
    Settings::default().save(&store).unwrap();
    let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    sql.execute(
        "UPDATE state SET value=? WHERE owner=? AND kind=? AND key='saved'",
        rusqlite::params!["tru", OWNER, KIND],
    )
    .unwrap();
    assert!(Settings::read(&store).is_err());
    let value = Settings::repair(&store).expect("explicit recovery must handle invalid JSON text");
    assert!(value.global_enabled);
    let backup = store.values(OWNER, "shortcut-recovery-v1").unwrap();
    assert_eq!(backup.values().next().unwrap()["saved_raw"], "tru");
}
#[test]
fn entire_map_is_atomic_and_old_windows_cannot_overwrite_new_preferences() {
    let root = tempfile::tempdir().unwrap();
    let store = StateStore::open(root.path()).unwrap();
    let other = StateStore::open(root.path()).unwrap();
    let stale = Settings::read(&other).unwrap();
    let mut edited = stale.clone();
    edited
        .bind(
            Action::Next,
            Scope::Local,
            Some(Chord::new(vec![17, 89]).unwrap()),
        )
        .unwrap();
    edited.global_enabled = false;
    edited.media_enabled = false;
    let saved = edited.save(&store).unwrap();
    assert!(stale.save(&other).is_err());
    assert_eq!(Settings::read(&other).unwrap(), saved);
    let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_shortcuts BEFORE INSERT ON state WHEN NEW.kind='shortcut-settings-v1' AND NEW.key='current' BEGIN SELECT RAISE(ABORT,'synthetic refusal'); END;").unwrap();
    let mut rejected = saved.clone();
    rejected.global_enabled = true;
    assert!(rejected.save(&store).is_err());
    assert_eq!(Settings::read(&store).unwrap(), saved);
    drop(store);
    drop(other);
    assert_eq!(
        Settings::read(&StateStore::open(root.path()).unwrap()).unwrap(),
        saved
    );
}
#[test]
fn corruption_and_missing_rows_require_explicit_atomic_repair_with_raw_backup() {
    let root = tempfile::tempdir().unwrap();
    let store = StateStore::open(root.path()).unwrap();
    let old = Settings::default().save(&store).unwrap();
    let mut bad = serde_json::to_value(&old).unwrap();
    bad["bindings"]["next"]
        .as_object_mut()
        .unwrap()
        .remove("local");
    store.put(OWNER, KIND, "current", &bad).unwrap();
    assert!(Settings::read(&store).is_err());
    let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_repair BEFORE INSERT ON state WHEN NEW.kind='shortcut-settings-v1' BEGIN SELECT RAISE(ABORT,'synthetic refusal'); END;").unwrap();
    assert!(Settings::repair(&store).is_err());
    assert!(
        store
            .values(OWNER, "shortcut-recovery-v1")
            .unwrap()
            .is_empty()
    );
    sql.execute_batch("DROP TRIGGER reject_repair;").unwrap();
    let fixed = Settings::repair(&store).unwrap();
    assert_ne!(old.revision, fixed.revision);
    assert!(old.save(&store).is_err());
    assert!(Settings::repair(&store).is_err());
    let backups = store.values(OWNER, "shortcut-recovery-v1").unwrap();
    assert_eq!(backups.values().next().unwrap()["current"], bad);
    sql.execute_batch("DELETE FROM state WHERE owner='local-device' AND kind='shortcut-settings-v1' AND key='current';").unwrap();
    assert!(Settings::read(&store).is_err());
    let second = Settings::repair(&store).unwrap();
    assert_ne!(second.revision, fixed.revision);
    store.put(OWNER, KIND, "saved", &json!(false)).unwrap();
    assert!(Settings::read(&store).is_err());
    assert!(Settings::repair(&store).is_ok());
    assert_eq!(
        store.values(OWNER, "shortcut-recovery-v1").unwrap().len(),
        3
    );
}
#[test]
fn malformed_storage_never_enables_a_partial_or_ambiguous_map() {
    let root = tempfile::tempdir().unwrap();
    let store = StateStore::open(root.path()).unwrap();
    let saved = Settings::default().save(&store).unwrap();
    let baseline = serde_json::to_value(saved).unwrap();
    for variant in 0..5 {
        let mut raw = baseline.clone();
        match variant {
            0 => {
                raw["bindings"].as_object_mut().unwrap().remove("prev");
            }
            1 => raw["bindings"]["next"]["local"] = json!([17, 80]),
            2 => raw["global_enabled"] = json!("false"),
            3 => raw["bindings"]["next"]["local"] = json!([17, 17, 39]),
            _ => raw["revision"] = json!("initial"),
        }
        store.put(OWNER, KIND, "current", &raw).unwrap();
        assert!(Settings::read(&store).is_err(), "variant {variant}");
    }
}
