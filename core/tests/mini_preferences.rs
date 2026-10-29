use cumulus_core::{Error, preferences::mini::MiniPreferences as Preferences, storage::StateStore};
use serde_json::json;
const OWNER: &str = "local-device";
const KIND: &str = "mini-preferences-v1";
#[test]
fn malformed_json_can_be_backed_up_and_repaired_without_losing_raw_text() {
    let root = tempfile::tempdir().unwrap();
    let store = StateStore::open(root.path()).unwrap();
    Preferences::replace(&store, "initial", false).unwrap();
    let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    sql.execute(
        "UPDATE state SET value=? WHERE owner=? AND kind=? AND key='current'",
        rusqlite::params!["{broken", OWNER, KIND],
    )
    .unwrap();
    assert!(Preferences::read(&store).is_err());
    let value =
        Preferences::repair(&store).expect("explicit recovery must handle invalid JSON text");
    assert!(value.always_on_top);
    let backup = store.values(OWNER, "mini-recovery-v1").unwrap();
    assert_eq!(backup.values().next().unwrap()["current_raw"], "{broken");
}
#[test]
fn concurrent_windows_and_reopen_preserve_one_atomic_preference() {
    let root = tempfile::tempdir().unwrap();
    let first = StateStore::open(root.path()).unwrap();
    let second = StateStore::open(root.path()).unwrap();
    let initial = Preferences::read(&first).unwrap();
    assert!(initial.always_on_top);
    let disabled = Preferences::replace(&first, &initial.revision, false).unwrap();
    assert!(!disabled.always_on_top);
    assert!(Preferences::replace(&second, &initial.revision, true).is_err());
    assert_eq!(Preferences::read(&second).unwrap(), disabled);
    drop(first);
    drop(second);
    let reopened = StateStore::open(root.path()).unwrap();
    assert_eq!(Preferences::read(&reopened).unwrap(), disabled);
    let always_on_top = Preferences::replace(&reopened, &disabled.revision, true).unwrap();
    assert!(always_on_top.always_on_top && always_on_top.revision != disabled.revision);
}
#[test]
fn repair_backs_up_damage_without_revalidating_any_old_window() {
    let root = tempfile::tempdir().unwrap();
    let store = StateStore::open(root.path()).unwrap();
    let old = Preferences::replace(&store, "initial", false).unwrap();
    store
        .put(
            OWNER,
            KIND,
            "current",
            &json!({"revision":old.revision,"always_on_top":"false"}),
        )
        .unwrap();
    assert!(matches!(Preferences::read(&store), Err(Error::Json(_))));
    assert!(Preferences::replace(&store, &old.revision, true).is_err());
    let repaired = Preferences::repair(&store).unwrap();
    assert!(repaired.always_on_top && repaired.revision != old.revision);
    assert!(Preferences::replace(&store, &old.revision, false).is_err());
    assert!(Preferences::repair(&store).is_err());
    store
        .put(OWNER, KIND, "saved", &json!("broken-marker"))
        .unwrap();
    assert!(Preferences::read(&store).is_err());
    let repaired_again = Preferences::repair(&store).unwrap();
    assert_ne!(repaired_again.revision, repaired.revision);
    let backups = store.values(OWNER, "mini-recovery-v1").unwrap();
    assert_eq!(backups.len(), 2);
    assert!(backups.values().any(|v| v["saved"] == "broken-marker"));
    assert!(
        backups
            .values()
            .any(|v| v["current"]["always_on_top"] == "false")
    );
}
#[test]
fn missing_current_and_storage_failure_never_silently_restore_default() {
    let root = tempfile::tempdir().unwrap();
    let store = StateStore::open(root.path()).unwrap();
    let saved = Preferences::replace(&store, "initial", false).unwrap();
    let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    sql.execute_batch("CREATE TRIGGER no_media_change BEFORE INSERT ON state WHEN NEW.kind='mini-preferences-v1' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    assert!(Preferences::replace(&store, &saved.revision, true).is_err());
    assert_eq!(Preferences::read(&store).unwrap(), saved);
    sql.execute_batch("DROP TRIGGER no_media_change; DELETE FROM state WHERE owner='local-device' AND kind='mini-preferences-v1' AND key='current';").unwrap();
    assert!(Preferences::read(&store).is_err());
    sql.execute_batch("CREATE TRIGGER no_media_repair BEFORE INSERT ON state WHEN NEW.kind='mini-preferences-v1' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    assert!(Preferences::repair(&store).is_err());
    assert!(store.values(OWNER, "mini-recovery-v1").unwrap().is_empty());
    sql.execute_batch("DROP TRIGGER no_media_repair;").unwrap();
    assert!(Preferences::repair(&store).unwrap().always_on_top);
}
