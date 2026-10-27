use cumulus_core::{
    Error,
    account::AccountSession,
    cloud::library::*,
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
const LIST: &str = "/api/v1/cloud/get";
const DELETE: &str = "/api/cloud/del";
#[derive(Default)]
struct Model {
    rows: Mutex<Vec<Value>>,
    calls: Mutex<Vec<(String, Value)>>,
    page_fault: Mutex<Option<(usize, &'static str)>>,
    delete_mode: Mutex<&'static str>,
    gate: Mutex<Option<Arc<Gate>>>,
}
struct Gate {
    entered: tokio::sync::Notify,
    release: tokio::sync::Semaphore,
}
impl Model {
    fn count(&self, path: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == path)
            .count()
    }
}
impl Api for Model {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), payload.clone()));
            if path == LIST {
                let offset = payload["offset"].as_u64().unwrap() as usize;
                assert_eq!(payload["limit"], 200);
                let rows = self.rows.lock().unwrap();
                let mut response = json!({"code":200,"data":rows.iter().skip(offset).take(200).cloned().collect::<Vec<_>>(),"hasMore":offset+200<rows.len(),"size":"1234","maxSize":"1073741824"});
                if let Some((at, kind)) = *self.page_fault.lock().unwrap()
                    && at == offset
                {
                    match kind {
                        "network" => return Err(Error::Transport("fixture".into())),
                        "duplicate" => response["data"][0] = rows[0].clone(),
                        "capacity" => response["maxSize"] = json!(2048),
                        "empty" => {
                            response["data"] = json!([]);
                            response["hasMore"] = json!(true)
                        }
                        "flags" => {
                            response["more"] = json!(!response["hasMore"].as_bool().unwrap())
                        }
                        _ => panic!(),
                    }
                }
                return Ok(response);
            }
            assert_eq!(path, DELETE);
            let ids: Vec<String> =
                serde_json::from_str(payload["songIds"].as_str().unwrap()).unwrap();
            assert!(ids.len() <= 1000);
            let mode = *self.delete_mode.lock().unwrap();
            if mode == "reject" {
                return Ok(json!({"code":403}));
            }
            let succeeded = match mode {
                "unknown" => vec![],
                "partial" => ids[..ids.len() - 1].to_vec(),
                _ => ids.clone(),
            };
            self.rows
                .lock()
                .unwrap()
                .retain(|row| !succeeded.contains(&row["songId"].to_string()));
            let gate = self.gate.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.entered.notify_one();
                gate.release.acquire().await.unwrap().forget();
            }
            if ["lost", "unknown"].contains(&mode) {
                return Err(Error::Transport("fixture lost ACK".into()));
            }
            Ok(
                json!({"code":200,"succIds":succeeded,"failIds":ids.into_iter().filter(|id|!succeeded.contains(id)).collect::<Vec<_>>()}),
            )
        })
    }
}
struct Fixture {
    root: tempfile::TempDir,
    store: Arc<StateStore>,
    session: Arc<AccountSession>,
    api: Arc<Model>,
    service: CloudLibrary,
}
fn rows(n: usize) -> Vec<Value> {
    (1..=n).map(|i|json!({"songId":i,"fileName":format!("file-{i}.flac"),"fileSize":123,"addTime":i,"bitrate":320000,
    "songName":"Local title","artist":"Local artist","album":"Local album","simpleSong":{"name":format!("Song {i}"),"id":999,"dt":123456}})).collect()
}
impl Fixture {
    fn new(n: usize) -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let api = Arc::new(Model::default());
        *api.rows.lock().unwrap() = rows(n);
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        let service = CloudLibrary::new(session.clone()).unwrap();
        Self {
            root,
            store,
            session,
            api,
            service,
        }
    }
    fn reopened(&self) -> CloudLibrary {
        let store = Arc::new(StateStore::open(self.root.path()).unwrap());
        let session = Arc::new(AccountSession::new(self.api.clone(), Some(store)));
        session
            .adopt(
                self.api.clone(),
                json!({"userId":1}),
                &session.token().unwrap(),
            )
            .unwrap();
        CloudLibrary::new(session).unwrap()
    }
}

#[tokio::test]
async fn complete_paging_capacity_normalization_and_reopen_preserve_order_and_cloud_identity() {
    let f = Fixture::new(205);
    assert!(f.service.cached().unwrap().is_none());
    let snapshot = f.service.refresh().await.unwrap();
    assert_eq!(snapshot.items.len(), 205);
    assert_eq!(snapshot.size, Some(1234));
    assert_eq!(snapshot.max_size, Some(1073741824));
    assert_eq!(snapshot.items[0].song_id, "1");
    assert_eq!(snapshot.items[204].song_id, "205");
    assert_eq!(snapshot.items[0].song["id"], "1");
    assert_eq!(snapshot.items[0].song["ar"][0]["name"], "Local artist");
    assert_eq!(snapshot.items[0].song["al"]["name"], "Local album");
    assert_eq!(f.api.count(LIST), 2);
    assert_eq!(
        f.reopened().cached().unwrap().unwrap().revision,
        snapshot.revision
    );
    assert_eq!(
        f.service.refresh().await.unwrap().revision,
        snapshot.revision
    );
}

#[tokio::test]
async fn image_cdn_rotation_does_not_change_revision_but_resource_metadata_and_membership_do() {
    let f = Fixture::new(2);
    f.api.rows.lock().unwrap()[0]["simpleSong"]["al"] =
        json!({"name":"Album","picUrl":"https://p3.music.126.net/fixture.jpg"});
    let before = f.service.refresh().await.unwrap();
    f.api.rows.lock().unwrap()[0]["simpleSong"]["al"]["picUrl"] =
        json!("https://p4.music.126.net/fixture.jpg");
    let rotated = f.service.refresh().await.unwrap();
    assert_eq!(before.revision, rotated.revision);
    assert_ne!(
        before.items[0].song["al"]["picUrl"],
        rotated.items[0].song["al"]["picUrl"]
    );
    assert_eq!(
        f.reopened().cached().unwrap().unwrap().revision,
        before.revision
    );
    f.api.rows.lock().unwrap()[0]["simpleSong"]["name"] = json!("Updated title");
    let edited = f.service.refresh().await.unwrap();
    assert_ne!(edited.revision, before.revision);
    f.api.rows.lock().unwrap().pop();
    assert_ne!(f.service.refresh().await.unwrap().revision, edited.revision);
}

#[tokio::test]
async fn legacy_snapshot_revision_is_readable_and_refresh_upgrades_without_losing_account_data() {
    use sha2::{Digest, Sha256};
    let f = Fixture::new(2);
    f.api.rows.lock().unwrap()[0]["simpleSong"]["al"] =
        json!({"name":"Album","picUrl":"https://p3.music.126.net/fixture.jpg"});
    let mut before = f.service.refresh().await.unwrap();
    let stable = before.revision.clone();
    let mut legacy = json!([before.items, before.size, before.max_size]);
    legacy.sort_all_objects();
    before.revision = hex::encode(Sha256::digest(serde_json::to_vec(&legacy).unwrap()));
    assert_ne!(stable, before.revision);
    f.store
        .put(
            "1",
            "cloud_library_v1",
            "snapshot",
            &serde_json::to_value(&before).unwrap(),
        )
        .unwrap();
    assert_eq!(
        f.reopened().cached().unwrap().unwrap().revision,
        before.revision
    );
    let upgraded = f.reopened().refresh().await.unwrap();
    assert_eq!(upgraded.revision, stable);
    assert_eq!(upgraded.items.len(), 2);
    assert_eq!(f.api.count(DELETE), 0);
}

#[tokio::test]
async fn bad_or_incomplete_pages_never_replace_the_previous_complete_snapshot() {
    for kind in ["network", "duplicate", "capacity", "empty", "flags"] {
        let f = Fixture::new(205);
        let before = f.service.refresh().await.unwrap();
        f.api.rows.lock().unwrap()[0]["songName"] = json!("Changed local fallback");
        *f.api.page_fault.lock().unwrap() = Some((200, kind));
        assert!(f.service.refresh().await.is_err());
        assert_eq!(
            f.reopened().cached().unwrap().unwrap().revision,
            before.revision
        );
    }
}

#[tokio::test]
async fn partial_deletion_requires_authoritative_readback_and_retains_unselected_items() {
    let f = Fixture::new(5);
    let before = f.service.refresh().await.unwrap();
    *f.api.delete_mode.lock().unwrap() = "partial";
    let op = f
        .service
        .delete(&["2".into(), "3".into(), "4".into()], &before.revision)
        .await
        .unwrap();
    assert_eq!(op.phase, DeletePhase::Partial);
    assert_eq!(op.removed, ["2", "3"]);
    assert_eq!(op.remaining, ["4"]);
    let snapshot = f.reopened().cached().unwrap().unwrap();
    assert_eq!(
        snapshot
            .items
            .iter()
            .map(|v| v.song_id.as_str())
            .collect::<Vec<_>>(),
        ["1", "4", "5"]
    );
    let calls = f.api.count(DELETE);
    f.reopened().reconcile_delete(&op.id).await.unwrap();
    assert_eq!(f.api.count(DELETE), calls);
}

#[tokio::test]
async fn unknown_delete_is_never_replayed_and_readonly_observation_can_confirm_or_retain_uncertainty()
 {
    for mode in ["lost", "unknown"] {
        let f = Fixture::new(3);
        let before = f.service.refresh().await.unwrap();
        *f.api.delete_mode.lock().unwrap() = mode;
        let op = f
            .service
            .delete(&["2".into()], &before.revision)
            .await
            .unwrap();
        if mode == "lost" {
            assert_eq!(op.phase, DeletePhase::Confirmed);
        } else {
            assert_eq!(op.phase, DeletePhase::Uncertain);
            assert!(matches!(
                f.reopened().delete(&["2".into()], &before.revision).await,
                Err(Error::MutationUncertain)
            ));
            assert_eq!(
                f.reopened().reconcile_delete(&op.id).await.unwrap().phase,
                DeletePhase::Uncertain
            );
            f.api.rows.lock().unwrap().retain(|row| row["songId"] != 2);
            assert_eq!(
                f.reopened().reconcile_delete(&op.id).await.unwrap().phase,
                DeletePhase::Confirmed
            );
        }
        assert_eq!(f.api.count(DELETE), 1);
    }
}

#[tokio::test]
async fn unknown_first_chunk_stops_later_chunks_and_reconciliation_does_not_send_unstarted_work() {
    let f = Fixture::new(1001);
    let before = f.service.refresh().await.unwrap();
    *f.api.delete_mode.lock().unwrap() = "unknown";
    let ids = before
        .items
        .iter()
        .map(|v| v.song_id.clone())
        .collect::<Vec<_>>();
    let op = f.service.delete(&ids, &before.revision).await.unwrap();
    assert_eq!(op.phase, DeletePhase::Uncertain);
    assert_eq!(f.api.count(DELETE), 1);
    f.api
        .rows
        .lock()
        .unwrap()
        .retain(|row| row["songId"] == 1001);
    let op = f.reopened().reconcile_delete(&op.id).await.unwrap();
    assert_eq!(op.phase, DeletePhase::Partial);
    assert_eq!(op.remaining, ["1001"]);
    assert_eq!(f.api.count(DELETE), 1);
}

#[tokio::test]
async fn changed_revision_invalid_selection_and_write_ahead_failure_send_no_delete() {
    let f = Fixture::new(3);
    let before = f.service.refresh().await.unwrap();
    for ids in [vec![], vec!["2".into(), "2".into()], vec!["99".into()]] {
        assert!(f.service.delete(&ids, &before.revision).await.is_err());
    }
    f.api.rows.lock().unwrap()[0]["fileSize"] = json!(456);
    assert!(matches!(
        f.service.delete(&["2".into()], &before.revision).await,
        Err(Error::QueueChanged)
    ));
    let current = f.service.cached().unwrap().unwrap();
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_intent BEFORE INSERT ON state WHEN NEW.kind='cloud_deletions_v1' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(matches!(
        f.service.delete(&["2".into()], &current.revision).await,
        Err(Error::Storage(_))
    ));
    assert_eq!(f.api.count(DELETE), 0);
}

#[tokio::test]
async fn failed_ack_commit_and_same_uid_relogin_preserve_unknown_journal_for_readonly_recovery() {
    let f = Fixture::new(3);
    let before = f.service.refresh().await.unwrap();
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_ack BEFORE INSERT ON state WHEN NEW.kind='cloud_deletions_v1' AND json_extract(NEW.value,'$.batches[0].phase')='acknowledged' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(matches!(
        f.service.delete(&["2".into()], &before.revision).await,
        Err(Error::Storage(_))
    ));
    db.execute_batch("DROP TRIGGER fail_ack;").unwrap();
    let op = f.reopened().operations().unwrap().pop().unwrap();
    assert_eq!(
        f.reopened().reconcile_delete(&op.id).await.unwrap().phase,
        DeletePhase::Confirmed
    );
    assert_eq!(f.api.count(DELETE), 1);

    let f = Fixture::new(3);
    let before = f.service.refresh().await.unwrap();
    let gate = Arc::new(Gate {
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Semaphore::new(0),
    });
    *f.api.gate.lock().unwrap() = Some(gate.clone());
    let service = f.service.clone();
    let worker = tokio::spawn(async move { service.delete(&["2".into()], &before.revision).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), gate.entered.notified())
        .await
        .unwrap();
    assert!(matches!(
        f.service.refresh().await,
        Err(Error::MutationBusy)
    ));
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":1}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    gate.release.add_permits(1);
    assert!(matches!(worker.await.unwrap(), Err(Error::StaleResponse)));
    let reopened = f.reopened();
    let op = reopened.operations().unwrap().pop().unwrap();
    assert_eq!(
        reopened.reconcile_delete(&op.id).await.unwrap().phase,
        DeletePhase::Confirmed
    );
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    let other = CloudLibrary::new(f.session.clone()).unwrap();
    assert!(other.cached().unwrap().is_none());
    assert!(other.operations().unwrap().is_empty());
    assert!(other.operation(&op.id).is_err());
}
