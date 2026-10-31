//! Only the synthetic UX host can install failures in its isolated database.
pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_VOLUME_UI_PROOF").is_some()
}
pub fn apply(store: &cumulus_core::storage::StateStore, kind: &str) {
    let sql = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    sql.execute_batch(match kind {
        "reject" => "CREATE TRIGGER reject_volume_insert BEFORE INSERT ON state WHEN NEW.owner='2' AND NEW.kind='playback' AND NEW.key='preferences' BEGIN SELECT RAISE(ABORT,'synthetic volume storage failure'); END;
                     CREATE TRIGGER reject_volume_update BEFORE UPDATE ON state WHEN NEW.owner='2' AND NEW.kind='playback' AND NEW.key='preferences' BEGIN SELECT RAISE(ABORT,'synthetic volume storage failure'); END;",
        "allow" => "DROP TRIGGER reject_volume_insert; DROP TRIGGER reject_volume_update;",
        _ => panic!("unknown synthetic volume fixture"),
    }).unwrap();
}
