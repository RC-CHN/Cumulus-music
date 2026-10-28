use cumulus_core::{Error, storage::StateStore};
use serde_json::json;
use std::{
    sync::{Arc, Barrier},
    thread,
};

#[test]
fn account_isolation_reopen_and_legacy_schema() {
    let tmp = tempfile::tempdir().unwrap();
    // Frozen SQLite file created by the archived Python StateStore, with
    // synthetic owners only. Reading it exercises actual on-disk compatibility.
    std::fs::write(
        tmp.path().join("core.sqlite3"),
        include_bytes!("fixtures/python-state.sqlite3"),
    )
    .unwrap();
    let store = StateStore::open(tmp.path()).unwrap();
    assert_eq!(
        store.get("101", "playlist", "2").unwrap(),
        Some(json!({"name":"甲","ids":[1,"2"]}))
    );
    store
        .put("202", "playlist", "2", &json!({"name":"乙"}))
        .unwrap();
    drop(store);
    let store = StateStore::open(tmp.path()).unwrap();
    assert_eq!(
        store.values("202", "playlist").unwrap()["2"],
        json!({"name":"乙"})
    );
    store.clear_owner("101").unwrap();
    assert_eq!(store.get("101", "playlist", "2").unwrap(), None);
    assert!(store.get("202", "playlist", "2").unwrap().is_some());
    store.delete("202", "playlist", "2").unwrap();
    assert!(store.values("202", "playlist").unwrap().is_empty());
}

#[test]
fn multi_record_failure_rolls_back_and_connection_remains_usable() {
    let tmp = tempfile::tempdir().unwrap();
    let store = StateStore::open(tmp.path()).unwrap();
    store
        .put("1", "job", "old", &json!({"status":"paused"}))
        .unwrap();
    let result: cumulus_core::Result<()> = store.transaction(|tx| {
        tx.delete("1", "job", "old")?;
        tx.put("2", "job", "new", &json!({"status":"running"}))?;
        Err(Error::State("synthetic publish failure"))
    });
    assert!(result.is_err());
    assert_eq!(
        store.get("1", "job", "old").unwrap(),
        Some(json!({"status":"paused"}))
    );
    assert_eq!(store.get("2", "job", "new").unwrap(), None);
    let rows = store.transaction(|tx| tx.rows("job")).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].owner, "1");
    store.put("1", "job", "after", &json!(true)).unwrap();
}

#[test]
fn independent_connections_do_not_lose_read_modify_write_updates() {
    let tmp = tempfile::tempdir().unwrap();
    let first = StateStore::open(tmp.path()).unwrap();
    first.put("1", "counter", "n", &json!(0)).unwrap();
    let second = StateStore::open(tmp.path()).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = [first, second]
        .into_iter()
        .map(|store| {
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                for _ in 0..40 {
                    store
                        .transaction(|tx| {
                            let n = tx.get("1", "counter", "n")?.unwrap().as_u64().unwrap();
                            thread::yield_now();
                            tx.put("1", "counter", "n", &json!(n + 1))
                        })
                        .unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(
        StateStore::open(tmp.path())
            .unwrap()
            .get("1", "counter", "n")
            .unwrap(),
        Some(json!(80))
    );
}

#[cfg(unix)]
#[test]
fn newly_created_state_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let directory = tmp.path().join("private");
    let store = StateStore::open(&directory).unwrap();
    assert_eq!(
        std::fs::metadata(store.directory())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(directory.join("core.sqlite3"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn keyset_pages_are_bounded_isolated_and_resume_after_the_cursor_is_deleted() {
    let root = tempfile::tempdir().unwrap();
    let store = StateStore::open(root.path()).unwrap();
    store
        .transaction(|tx| {
            for key in ["", "a", "b", "d"] {
                tx.put("1", "history", key, &json!(key))?;
            }
            tx.put("2", "history", "c", &json!("other account"))?;
            tx.put("1", "other", "c", &json!("other namespace"))?;
            let first = tx.values_page("1", "history", None, 2)?;
            assert_eq!(
                first.keys().map(String::as_str).collect::<Vec<_>>(),
                vec!["", "a"]
            );
            tx.delete("1", "history", "a")?;
            let second = tx.values_page("1", "history", Some("a"), 2)?;
            assert_eq!(
                second.keys().map(String::as_str).collect::<Vec<_>>(),
                vec!["b", "d"]
            );
            assert!(tx.values_page("1", "history", Some("d"), 2)?.is_empty());
            assert!(tx.values_page("1", "history", None, 0).is_err());
            assert!(tx.values_page("1", "history", None, 1025).is_err());
            Ok(())
        })
        .unwrap();
}
