mod support;
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    download::offline::{OfflineLibrary, PublishedDownload},
    media::{
        integrity::{inspect_file, inspect_file_until},
        ncm::{NcmWriteOptions, write_file},
    },
    membership::{
        OfflineDenial, Quality,
        clock::{ClockReading, MembershipClock},
    },
    storage::StateStore,
};
use serde_json::json;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use support::{Reply, ScriptApi, gate};

struct Clock(Mutex<ClockReading>);
impl MembershipClock for Clock {
    fn now(&self) -> Result<ClockReading> {
        Ok(*self.0.lock().unwrap())
    }
}
fn clock() -> Arc<Clock> {
    Arc::new(Clock(Mutex::new(ClockReading {
        wall_seconds: 1000.,
        monotonic_seconds: 40.,
    })))
}
fn response(now: f64, expiry: f64) -> Reply {
    json!({"code":200,"data":{"userId":"1","now":now,"associator":{"expireTime":expiry,"vipLevel":7}}}).into()
}
async fn session(
    store: Arc<StateStore>,
    replies: impl IntoIterator<Item = Reply>,
) -> (Arc<AccountSession>, Arc<ScriptApi>) {
    let mut list = vec![json!({"code":200,"profile":{"userId":1}}).into()];
    list.extend(replies);
    let api = Arc::new(ScriptApi::new(list));
    let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
    session.refresh().await.unwrap();
    (session, api)
}
fn task(root: &Path, name: &str, encrypted: bool) -> PublishedDownload {
    let path = root.join(name);
    let source = root.join(format!("{name}-raw"));
    let seed = include_bytes!("fixtures/tag_audio/seed.flac");
    if encrypted {
        std::fs::write(&source, seed).unwrap();
        write_file(&source, &path, NcmWriteOptions::default()).unwrap();
    } else {
        std::fs::write(&path, seed).unwrap();
    }
    PublishedDownload {
        task_id: format!("task-{name}"),
        song_id: "123".into(),
        requested_quality: Quality::HiRes,
        target: path.clone(),
        song: json!({"id":123,"privilege":{"flag":4,"fee":1}}),
        resource: json!({"id":123,"level":"lossless","flag":4,"fee":1,"payed":0}),
        encrypted,
        integrity: inspect_file(path).unwrap(),
    }
}

#[tokio::test]
async fn plaintext_publication_is_account_scoped_durable_idempotent_and_hash_bound() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
    let (session, api) = session(store.clone(), []).await;
    let token = session.token().unwrap();
    let lib = OfflineLibrary::new(session.clone()).unwrap();
    let t = task(root.path(), "plain.flac", false);
    let first = lib.register(&token, &t).unwrap();
    assert_eq!(first.quality, Quality::Lossless);
    assert_eq!(lib.register(&token, &t).unwrap(), first);
    let check = lib.check(&token, &t.target, true).await.unwrap();
    assert!(check.access.can_play && check.integrity_verified);
    assert_eq!(api.calls().len(), 1);
    let reopened = OfflineLibrary::new(session.clone()).unwrap();
    assert_eq!(reopened.records(&token).unwrap(), vec![first.clone()]);
    let other = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    other
        .adopt(api.clone(), json!({"userId":2}), &other.token().unwrap())
        .unwrap();
    let second = OfflineLibrary::new(other.clone()).unwrap();
    assert!(
        second
            .check(&other.token().unwrap(), &t.target, false)
            .await
            .is_err()
    );
    assert!(second.records(&other.token().unwrap()).unwrap().is_empty());
    let mut data = std::fs::read(&t.target).unwrap();
    data[100] ^= 1;
    std::fs::write(&t.target, &data).unwrap();
    assert!(matches!(
        lib.check(&token, &t.target, false).await,
        Err(Error::Media(_))
    ));
    assert!(lib.register(&token, &t).is_err());
    let mut wrong = t;
    wrong.resource["id"] = json!(999);
    assert!(lib.register(&token, &wrong).is_err());
    reopened.forget(&token, &first).unwrap();
    assert!(reopened.records(&token).unwrap().is_empty());
    assert_eq!(std::fs::read(&wrong.target).unwrap(), data);
}

#[tokio::test]
async fn encrypted_records_use_member_clock_actual_quality_and_preserved_album_flags() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
    let (session, api) = session(
        store,
        [response(2000000., 2001000.), response(2002000., 2010000.)],
    )
    .await;
    let token = session.token().unwrap();
    let clock = clock();
    let lib = OfflineLibrary::with_clock(session.clone(), clock.clone()).unwrap();
    let t = task(root.path(), "vip.ncm", true);
    lib.register(&token, &t).unwrap();
    assert!(lib.check(&token, &t.target, false).await.is_err());
    assert_eq!(api.calls().len(), 1);
    assert!(
        lib.check(&token, &t.target, true)
            .await
            .unwrap()
            .access
            .can_play
    );
    clock.0.lock().unwrap().monotonic_seconds += 1.;
    let denied = lib.check(&token, &t.target, false).await.unwrap();
    assert_eq!(denied.access.reason, Some(OfflineDenial::Vip));
    assert!(!denied.integrity_verified);
    assert!(
        lib.check(&token, &t.target, true)
            .await
            .unwrap()
            .integrity_verified
    );
    let mut album = task(root.path(), "album.ncm", true);
    album.song["privilege"]["flag"] = json!(8196);
    album.resource["payed"] = json!(6);
    let r = lib.register(&token, &album).unwrap();
    assert_eq!(r.privilege["flag"], 8196);
    assert_eq!(
        lib.check(&token, &album.target, false)
            .await
            .unwrap()
            .access
            .reason,
        Some(OfflineDenial::Album)
    );
    let mut plus = task(root.path(), "plus.ncm", true);
    plus.resource["level"] = json!("sky");
    plus.resource["payed"] = json!(6);
    lib.register(&token, &plus).unwrap();
    assert_eq!(
        lib.check(&token, &plus.target, false)
            .await
            .unwrap()
            .access
            .reason,
        Some(OfflineDenial::Svip)
    );
    let reopened = OfflineLibrary::with_clock(session, clock).unwrap();
    assert!(
        reopened
            .check(&token, &t.target, false)
            .await
            .unwrap()
            .access
            .can_play
    );
    assert_eq!(api.calls().len(), 3);
}

#[tokio::test]
async fn pending_renewal_locks_across_instances_and_rejects_late_account_result() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
    let (pending, send) = gate();
    let (session, api) = session(store.clone(), [pending]).await;
    let token = session.token().unwrap();
    let lib = Arc::new(OfflineLibrary::with_clock(session.clone(), clock()).unwrap());
    let task = task(root.path(), "pending.ncm", true);
    let record = lib.register(&token, &task).unwrap();
    let operation = {
        let lib = lib.clone();
        let token = token.clone();
        let path = task.target.clone();
        tokio::spawn(async move { lib.check(&token, path, true).await })
    };
    api.wait_for_calls(2).await;
    let other = OfflineLibrary::with_clock(session.clone(), clock()).unwrap();
    assert!(matches!(
        other.register(&token, &task),
        Err(Error::MutationBusy)
    ));
    session
        .adopt(api.clone(), json!({"userId":2}), &token)
        .unwrap();
    send.send(Ok(
        json!({"code":200,"data":{"userId":1,"now":2000000,"associator":{"expireTime":3000000}}}),
    ))
    .unwrap();
    assert!(matches!(
        operation.await.unwrap(),
        Err(Error::StaleResponse)
    ));
    assert!(other.records(&session.token().unwrap()).unwrap().is_empty());
    assert_eq!(store.values("1", "offline_music_v1").unwrap().len(), 1);
    assert!(store.get("2", "membership", "current").unwrap().is_none());
    session
        .adopt(api, json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    assert_eq!(
        other
            .record(&session.token().unwrap(), &task.target)
            .unwrap(),
        Some(record)
    );
}

#[tokio::test]
async fn aborted_checks_release_locks_and_failed_storage_never_reports_registration() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let store = Arc::new(StateStore::open(&state).unwrap());
    let (pending, _send) = gate();
    let (session, api) = session(store, [pending, response(2000000., 3000000.)]).await;
    let token = session.token().unwrap();
    let lib = Arc::new(OfflineLibrary::with_clock(session.clone(), clock()).unwrap());
    let t = task(root.path(), "abort.ncm", true);
    let db = rusqlite::Connection::open(state.join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_offline BEFORE INSERT ON state WHEN NEW.kind='offline_music_v1' BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert!(matches!(lib.register(&token, &t), Err(Error::Storage(_))));
    assert!(lib.records(&token).unwrap().is_empty());
    assert!(t.target.exists());
    db.execute_batch("DROP TRIGGER fail_offline;").unwrap();
    let record = lib.register(&token, &t).unwrap();
    let operation = {
        let lib = lib.clone();
        let token = token.clone();
        let path = t.target.clone();
        tokio::spawn(async move { lib.check(&token, path, true).await })
    };
    api.wait_for_calls(2).await;
    operation.abort();
    assert!(operation.await.unwrap_err().is_cancelled());
    assert!(
        lib.check(&token, &t.target, true)
            .await
            .unwrap()
            .integrity_verified
    );
    assert_eq!(lib.record(&token, &t.target).unwrap(), Some(record));
    assert!(matches!(
        inspect_file_until(&t.target, || true),
        Err(Error::Cancelled)
    ));
    std::fs::remove_file(&t.target).unwrap();
    assert_eq!(lib.records(&token).unwrap().len(), 1);
}

#[tokio::test]
async fn invalid_encrypted_container_or_digest_cannot_create_a_record() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
    let (session, _) = session(store, []).await;
    let token = session.token().unwrap();
    let lib = OfflineLibrary::new(session).unwrap();
    let mut t = task(root.path(), "invalid.flac", false);
    t.encrypted = true;
    assert!(lib.register(&token, &t).is_err());
    t.encrypted = false;
    t.integrity.sha256 = "0".repeat(64);
    assert!(lib.register(&token, &t).is_err());
    assert!(lib.records(&token).unwrap().is_empty());
}

#[tokio::test]
async fn entitlement_is_rechecked_after_integrity_io() {
    struct Expiring(std::sync::atomic::AtomicU32);
    impl MembershipClock for Expiring {
        fn now(&self) -> Result<ClockReading> {
            let tick = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ClockReading {
                wall_seconds: 1000.,
                monotonic_seconds: if tick < 2 { 40. } else { 42. },
            })
        }
    }
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
    let (session, api) = session(store, [response(2000000., 2001000.)]).await;
    let token = session.token().unwrap();
    let lib = OfflineLibrary::with_clock(
        session,
        Arc::new(Expiring(std::sync::atomic::AtomicU32::new(0))),
    )
    .unwrap();
    let t = task(root.path(), "expiry.ncm", true);
    lib.register(&token, &t).unwrap();
    lib.refresh_membership(&token).await.unwrap();
    let check = lib.check(&token, &t.target, false).await.unwrap();
    assert!(check.integrity_verified);
    assert_eq!(check.access.reason, Some(OfflineDenial::Vip));
    assert_eq!(api.calls().len(), 2);
}
