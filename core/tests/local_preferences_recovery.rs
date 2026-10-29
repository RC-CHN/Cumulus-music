//! Exercise unreadable SQLite text, not merely a JSON object with wrong fields.
use cumulus_core::{
    Result,
    playback::{output::OutputPreferences, preferences::StartupPreferences},
    preferences::window::WindowPreferences,
    storage::StateStore,
};
use serde_json::json;

struct Domain {
    kind: &'static str,
    backup: &'static str,
    save: fn(&StateStore, u64) -> Result<u64>,
    read: fn(&StateStore) -> Result<u64>,
    repair: fn(&StateStore) -> Result<u64>,
}
const STARTUP: Domain = Domain {
    kind: "playback-startup-preferences-v1",
    backup: "playback-startup-recovery-v1",
    save: |s, r| StartupPreferences::replace(s, r, Default::default()).map(|v| v.revision),
    read: |s| StartupPreferences::read(s).map(|v| v.revision),
    repair: |s| StartupPreferences::repair(s).map(|v| v.revision),
};
const OUTPUT: Domain = Domain {
    kind: "audio-output-preferences-v1",
    backup: "audio-output-recovery-v1",
    save: |s, r| OutputPreferences::replace(s, r, Default::default()).map(|v| v.revision),
    read: |s| OutputPreferences::read(s).map(|v| v.revision),
    repair: |s| OutputPreferences::repair(s).map(|v| v.revision),
};
const WINDOW: Domain = Domain {
    kind: "window-behavior-preferences-v1",
    backup: "window-behavior-recovery-v1",
    save: |s, r| WindowPreferences::replace(s, r, Default::default()).map(|v| v.revision),
    read: |s| WindowPreferences::read(s).map(|v| v.revision),
    repair: |s| WindowPreferences::repair(s).map(|v| v.revision),
};

fn recovery(domain: Domain) {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path()).unwrap();
    let saved = (domain.save)(&store, 0).unwrap();
    let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    sql.execute(
        "UPDATE state SET value=? WHERE owner='local-device' AND kind=? AND key='current'",
        rusqlite::params!["{unparseable", domain.kind],
    )
    .unwrap();
    assert!((domain.read)(&store).is_err());
    assert!((domain.save)(&store, saved).is_err());
    let next = (domain.repair)(&store).expect("explicit repair must preserve malformed text");
    assert!(next > saved);
    assert!(
        (domain.repair)(&store).is_err(),
        "healthy settings cannot be reset by stale repair"
    );
    assert!((domain.save)(&store, saved).is_err());
    let backup = store.values("local-device", domain.backup).unwrap();
    assert_eq!(backup.len(), 1);
    assert_eq!(
        backup.values().next().unwrap()["current_raw"],
        "{unparseable"
    );
    drop(store);
    let reopened = StateStore::open(dir.path()).unwrap();
    assert_eq!((domain.read)(&reopened).unwrap(), next);
}
#[test]
fn malformed_startup_settings_repair_preserves_text_and_retires_old_windows() {
    recovery(STARTUP);
}
#[test]
fn malformed_audio_output_repair_preserves_text_and_retires_old_windows() {
    recovery(OUTPUT);
}
#[test]
fn malformed_window_settings_repair_preserves_text_and_retires_old_windows() {
    recovery(WINDOW);
}

#[test]
fn lost_saved_startup_record_requires_repair_instead_of_becoming_a_fresh_installation() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::open(dir.path()).unwrap();
    let saved = (STARTUP.save)(&store, 0).unwrap();
    store
        .delete("local-device", STARTUP.kind, "current")
        .unwrap();
    assert!(
        (STARTUP.read)(&store).is_err(),
        "lost saved settings must not look like defaults"
    );
    assert!(
        (STARTUP.save)(&store, 0).is_err(),
        "an old fresh-installation window cannot overwrite damage"
    );
    let next = (STARTUP.repair)(&store).unwrap();
    assert!(next > saved);
    assert_eq!(
        store
            .values("local-device", STARTUP.backup)
            .unwrap()
            .values()
            .next()
            .unwrap(),
        &json!({"missing":true})
    );
}

#[test]
fn repair_storage_failure_rolls_back_backup_counter_and_original_damage() {
    for domain in [STARTUP, OUTPUT, WINDOW] {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path()).unwrap();
        let saved = (domain.save)(&store, 0).unwrap();
        let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
        sql.execute(
            "UPDATE state SET value=? WHERE owner='local-device' AND kind=? AND key='current'",
            rusqlite::params!["{unparseable", domain.kind],
        )
        .unwrap();
        sql.execute_batch("CREATE TRIGGER fail_repair BEFORE INSERT ON state WHEN NEW.key='current' BEGIN SELECT RAISE(ABORT,'synthetic write failure'); END;").unwrap();
        assert!((domain.repair)(&store).is_err());
        assert!(
            store
                .values("local-device", domain.backup)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store.get("local-device", domain.kind, "revision").unwrap(),
            Some(json!(saved))
        );
        let raw: String = sql
            .query_row(
                "SELECT value FROM state WHERE owner='local-device' AND kind=? AND key='current'",
                [domain.kind],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw, "{unparseable");
        sql.execute_batch("DROP TRIGGER fail_repair").unwrap();
        assert!((domain.repair)(&store).unwrap() > saved);
    }
}

#[test]
fn lost_or_damaged_revision_never_reuses_a_token_when_current_is_unreadable() {
    for domain in [STARTUP, OUTPUT, WINDOW] {
        for counter in [None, Some("\"broken\""), Some("18446744073709551615")] {
            let dir = tempfile::tempdir().unwrap();
            let store = StateStore::open(dir.path()).unwrap();
            (domain.save)(&store, 0).unwrap();
            let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
            sql.execute("UPDATE state SET value='{broken' WHERE owner='local-device' AND kind=? AND key='current'", [domain.kind]).unwrap();
            if let Some(text) = counter {
                sql.execute("UPDATE state SET value=? WHERE owner='local-device' AND kind=? AND key='revision'", rusqlite::params![text,domain.kind]).unwrap();
            } else {
                store
                    .delete("local-device", domain.kind, "revision")
                    .unwrap();
            }
            assert!(
                (domain.repair)(&store).is_err(),
                "unknown previous counter cannot safely restart at one"
            );
            assert!((domain.read)(&store).is_err());
            assert!(
                store
                    .values("local-device", domain.backup)
                    .unwrap()
                    .is_empty()
            );
        }
    }
}
