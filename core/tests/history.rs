use cumulus_core::{
    Error,
    account::AccountSession,
    history::{
        HistoryAction, HistoryApi, HistoryBoundary, HistoryKind, HistoryMutationPhase,
        HistoryRecord, HistoryService, RecordOrigin, merge_records,
    },
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};
use tokio::sync::Notify;
type Call = (String, Value, bool);
struct Backend {
    responses: Mutex<BTreeMap<String, Value>>,
    calls: Mutex<Vec<Call>>,
    hold: AtomicBool,
    hold_write: AtomicBool,
    write_error: AtomicU8,
    started: Notify,
    release: Notify,
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((path.into(), args, cache));
            let write = path.ends_with("/delete") || path.ends_with("/clear");
            if self.hold.load(Ordering::SeqCst) && path == "/api/play-record/song/list"
                || self.hold_write.load(Ordering::SeqCst) && write
            {
                self.started.notify_one();
                self.release.notified().await;
            }
            if write {
                match self.write_error.load(Ordering::SeqCst) {
                    1 => return Err(Error::Transport("lost reply".into())),
                    2 => {
                        return Err(Error::RequestNotSent(Box::new(Error::Response(
                            "prepared before transport",
                        ))));
                    }
                    _ => (),
                }
            }
            self.responses
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or(Error::Response("unexpected history fixture request"))
        })
    }
}
fn row(data: Value, time: u64) -> Value {
    json!({"data":data,"playTime":time,"multiTerminalInfo":{"os":"android","osText":"手机"}})
}
fn fixture() -> (
    tempfile::TempDir,
    Arc<StateStore>,
    Arc<Backend>,
    Arc<AccountSession>,
    Arc<HistoryService>,
) {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let api=Arc::new(Backend { responses:Mutex::new([
        ("/api/play-record/song/list",json!({"code":200,"data":{"list":[row(json!({"id":"90071992547409931234","name":"合成歌曲"}),20),row(json!({"id":2,"name":"歌曲二"}),10)]}})),
        ("/api/song/enhance/privilege",json!({"code":200,"data":[{"id":"90071992547409931234","pl":320000},{"id":"90071992547409931234","pl":999000},{"id":999,"pl":0}]})),
        ("/api/play-record/album/list",json!({"code":200,"data":{"list":[row(json!({"albumId":5,"albumName":"专辑"}),11)]}})),
        ("/api/play-record/playlist/list",json!({"code":200,"data":{"list":[row(json!({"id":6,"name":"歌单"}),12)]}})),
        ("/api/play-record/voice/list",json!({"code":200,"data":{"list":[row(json!({"pubDJProgramData":{"voiceId":7,"voiceName":"声音","mainSong":{"id":77}}}),13),row(json!({}),14)]}})),
        ("/api/play-record/djradio/list",json!({"code":200,"data":{"list":[row(json!({"resourceId":8,"name":"播客","resourceType":"DJ_RADIO"}),14),row(json!({"id":9,"name":"书","resourceType":"VOICE_BOOK"}),15)]}})),
        ("/api/play-record/voice/book/timeline/list",json!({"code":200,"data":{"data":{"todayRecords":[row(json!({"productId":10,"name":"书"}),30)],"yesterdayRecords":[],"theDayBeforeYesterdayRecords":[],"earlierRecords":[row(json!({"id":11,"name":"旧书"}),1)]}}})),
        ("/api/play-record/delete",json!({"code":200})),("/api/play-record/batch/delete",json!({"code":200})),("/api/play-record/clear",json!({"code":200})),
    ].into_iter().map(|(p,v)|(p.into(),v)).collect()), calls:Mutex::default(),hold:AtomicBool::new(false),hold_write:AtomicBool::new(false),write_error:AtomicU8::new(0),started:Notify::new(),release:Notify::new() });
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    session
        .adopt(api.clone(), json!({"userId":99}), &session.token().unwrap())
        .unwrap();
    let service = Arc::new(HistoryService::new(session.clone()).unwrap());
    (root, store, api, session, service)
}
#[tokio::test]
async fn six_official_lists_keep_identity_order_timeline_and_privilege_semantics() {
    let (_root, _store, api, session, service) = fixture();
    let token = session.token().unwrap();
    let mut pages = Vec::new();
    for kind in HistoryKind::ALL {
        assert!(service.snapshot_for(&token, kind).unwrap().remote.is_none());
        let page = service.refresh_for(&token, kind).await.unwrap();
        assert_eq!(
            serde_json::to_value(&page).unwrap(),
            serde_json::to_value(service.snapshot_for(&token, kind).unwrap()).unwrap()
        );
        pages.push(page);
    }
    assert_eq!(pages[0].entries.len(), 2);
    assert_eq!(pages[0].entries[0].id, "90071992547409931234");
    assert_eq!(pages[0].entries[0].resource["privilege"]["pl"], 999000);
    assert!(pages[0].entries[1].resource["privilege"].is_null());
    assert_eq!(pages[1].entries[0].id, "5");
    assert_eq!(pages[3].entries[0].id, "7");
    assert_eq!(pages[3].entries[0].resource["mainTrackId"], "77");
    assert_eq!(pages[4].entries.len(), 1);
    assert_eq!(pages[4].entries[0].id, "8");
    assert_eq!(
        pages[5]
            .entries
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        vec!["10", "11"]
    );
    assert_eq!(pages[5].entries[0].device.as_deref(), Some("手机"));
    assert_eq!(pages[0].entries[0].device.as_deref(), Some("android"));
    assert_eq!(
        *api.calls.lock().unwrap(),
        vec![
            (
                "/api/play-record/song/list".into(),
                json!({"limit":300}),
                false
            ),
            (
                "/api/song/enhance/privilege".into(),
                json!({"ids":"[\"90071992547409931234\",\"2\"]"}),
                false
            ),
            (
                "/api/play-record/album/list".into(),
                json!({"limit":100}),
                false
            ),
            (
                "/api/play-record/playlist/list".into(),
                json!({"limit":100}),
                false
            ),
            ("/api/play-record/voice/list".into(), json!({}), false),
            (
                "/api/play-record/djradio/list".into(),
                json!({"filterVoiceBook":true}),
                false
            ),
            (
                "/api/play-record/voice/book/timeline/list".into(),
                json!({}),
                false
            ),
        ]
    );
}
fn vector_record(value: &Value, origin: RecordOrigin) -> HistoryRecord {
    let mut resource = value.clone();
    for field in ["playtime", "osIcon", "playProgress", "isListened"] {
        resource.as_object_mut().unwrap().remove(field);
    }
    HistoryRecord {
        id: value["id"].as_str().unwrap().into(),
        played_at: value["playtime"].as_u64().unwrap(),
        resource,
        device: value["osIcon"].as_str().map(str::to_owned),
        origin,
        play_progress: value["playProgress"].as_f64(),
        listened: value["isListened"].as_bool(),
    }
}
fn vector_value(record: HistoryRecord) -> Value {
    let mut v = record.resource;
    v["playtime"] = json!(record.played_at);
    if let Some(device) = record.device {
        v["osIcon"] = json!(device);
    }
    if let Some(progress) = record.play_progress {
        v["playProgress"] = json!(progress as u64);
    }
    if let Some(listened) = record.listened {
        v["isListened"] = json!(listened);
    }
    v
}
#[test]
fn merges_match_executed_official_m542_vectors() {
    let vectors: Value = serde_json::from_str(include_str!("fixtures/history-merge.json")).unwrap();
    for case in vectors["cases"].as_array().unwrap() {
        let local = case["local"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| vector_record(v, RecordOrigin::Local))
            .collect::<Vec<_>>();
        let remote = case["remote"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| vector_record(v, RecordOrigin::Remote))
            .collect::<Vec<_>>();
        let before = local.clone();
        assert_eq!(
            json!(
                merge_records(&local, &remote)
                    .into_iter()
                    .map(vector_value)
                    .collect::<Vec<_>>()
            ),
            case["expected"],
            "{}",
            case["name"]
        );
        assert_eq!(
            local, before,
            "merging must not mutate persisted local metadata"
        );
    }
}
#[tokio::test]
async fn failed_and_malformed_refreshes_preserve_last_good_snapshot() {
    let (_root, _store, api, session, service) = fixture();
    let token = session.token().unwrap();
    let good = service
        .refresh_for(&token, HistoryKind::Song)
        .await
        .unwrap();
    for response in [
        json!({"code":500}),
        json!({"code":200,"data":{}}),
        json!({"code":200,"data":{"list":[row(json!({"id":0}),1)]}}),
        json!({"code":200,"data":{"list":[{"data":{"id":1},"playTime":-1}]}}),
    ] {
        api.responses
            .lock()
            .unwrap()
            .insert("/api/play-record/song/list".into(), response);
        assert!(
            service
                .refresh_for(&token, HistoryKind::Song)
                .await
                .is_err()
        );
        assert_eq!(
            service
                .snapshot_for(&token, HistoryKind::Song)
                .unwrap()
                .revision,
            good.revision
        );
    }
    api.responses.lock().unwrap().insert(
        "/api/play-record/song/list".into(),
        json!({"code":200,"data":{"list":[]}}),
    );
    let empty = service
        .refresh_for(&token, HistoryKind::Song)
        .await
        .unwrap();
    assert!(empty.entries.is_empty() && empty.remote.is_some());
    assert_ne!(empty.revision, good.revision);
}
#[tokio::test]
async fn local_history_is_bounded_persistent_merged_and_account_isolated() {
    let (root, store, api, session, service) = fixture();
    let token = session.token().unwrap();
    for kind in HistoryKind::ALL {
        for id in 1..=(kind.local_limit() + 2) {
            service
                .record_for(
                    &token,
                    kind,
                    HistoryRecord::local(kind, json!({"id":id,"name":"合成"}), id as u64).unwrap(),
                )
                .unwrap();
        }
        let page = service.snapshot_for(&token, kind).unwrap();
        assert_eq!(page.local.len(), kind.local_limit());
        assert_eq!(page.local.last().unwrap().id, "3");
        assert_eq!(page.local[0].played_at, (kind.local_limit() + 2) as u64);
    }
    service
        .record_for(
            &token,
            HistoryKind::Song,
            HistoryRecord::local(HistoryKind::Song, json!({"id":302,"name":"迟到"}), 1).unwrap(),
        )
        .unwrap();
    assert_eq!(
        service
            .snapshot_for(&token, HistoryKind::Song)
            .unwrap()
            .local[0]
            .resource["name"],
        "合成"
    );
    let mut voice =
        HistoryRecord::local(HistoryKind::Voice, json!({"id":7,"name":"本机声音"}), 8).unwrap();
    voice.play_progress = Some(42.5);
    service
        .record_for(&token, HistoryKind::Voice, voice)
        .unwrap();
    let page = service
        .refresh_for(&token, HistoryKind::Voice)
        .await
        .unwrap();
    assert_eq!(
        page.entries
            .iter()
            .find(|r| r.id == "7")
            .unwrap()
            .play_progress,
        Some(42.5)
    );
    service.set_sync_for(&token, false).unwrap();
    assert_eq!(
        service
            .snapshot_for(&token, HistoryKind::Voice)
            .unwrap()
            .entries,
        merge_records(&page.local, &[])
    );
    assert!(
        service
            .refresh_for(&token, HistoryKind::Voice)
            .await
            .is_err()
    );
    let reopened = Arc::new(AccountSession::new(
        api.clone(),
        Some(Arc::new(StateStore::open(root.path()).unwrap())),
    ));
    reopened
        .adopt(
            api.clone(),
            json!({"userId":99}),
            &reopened.token().unwrap(),
        )
        .unwrap();
    let again = HistoryService::new(reopened.clone())
        .unwrap()
        .snapshot_for(&reopened.token().unwrap(), HistoryKind::Voice)
        .unwrap();
    assert!(!again.sync_enabled && again.remote.is_some());
    assert_eq!(
        again.local,
        service
            .snapshot_for(&token, HistoryKind::Voice)
            .unwrap()
            .local
    );
    session.adopt(api, json!({"userId":100}), &token).unwrap();
    assert!(matches!(
        service.snapshot_for(&token, HistoryKind::Voice),
        Err(Error::StaleResponse)
    ));
    assert!(
        service
            .snapshot_for(&session.token().unwrap(), HistoryKind::Voice)
            .unwrap()
            .entries
            .is_empty()
    );
    assert!(store.values("99", "play-history-v1").unwrap().len() > 6);
}
#[tokio::test]
async fn account_change_and_inflight_sync_disable_drop_late_history() {
    let (_root, store, api, session, service) = fixture();
    let token = session.token().unwrap();
    api.hold.store(true, Ordering::SeqCst);
    let worker = tokio::spawn({
        let service = service.clone();
        let token = token.clone();
        async move { service.refresh_for(&token, HistoryKind::Song).await }
    });
    api.started.notified().await;
    assert!(matches!(
        service.refresh_for(&token, HistoryKind::Song).await,
        Err(Error::MutationBusy)
    ));
    service.set_sync_for(&token, false).unwrap();
    api.release.notify_one();
    assert!(matches!(worker.await.unwrap(), Err(Error::StaleResponse)));
    assert!(
        service
            .snapshot_for(&token, HistoryKind::Song)
            .unwrap()
            .remote
            .is_none()
    );
    service.set_sync_for(&token, true).unwrap();
    let worker = tokio::spawn({
        let service = service.clone();
        let token = token.clone();
        async move { service.refresh_for(&token, HistoryKind::Song).await }
    });
    api.started.notified().await;
    session
        .adopt(api.clone(), json!({"userId":100}), &token)
        .unwrap();
    api.release.notify_one();
    assert!(matches!(worker.await.unwrap(), Err(Error::StaleResponse)));
    assert!(
        store
            .get("99", "play-history-v1", "remote:song")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get("100", "play-history-v1", "remote:song")
            .unwrap()
            .is_none()
    );
}
#[test]
fn guest_local_files_are_distinct_and_never_authorize_remote_history() {
    let (_root, store, api, _session, _service) = fixture();
    let guest = Arc::new(AccountSession::new(api.clone(), Some(store)));
    let service = HistoryService::new(guest.clone()).unwrap();
    let token = guest.token().unwrap();
    let record = HistoryRecord::local(
        HistoryKind::Song,
        json!({"local_id":"123","name":"本地文件"}),
        1,
    )
    .unwrap();
    assert_eq!(record.id, "local:123");
    service
        .record_for(&token, HistoryKind::Song, record)
        .unwrap();
    assert_eq!(
        service
            .snapshot_for(&token, HistoryKind::Song)
            .unwrap()
            .entries
            .len(),
        1
    );
    guest
        .adopt(api.clone(), json!({"userId":101}), &token)
        .unwrap();
    assert!(
        service
            .snapshot_for(&guest.token().unwrap(), HistoryKind::Song)
            .unwrap()
            .entries
            .is_empty()
    );
    assert!(api.calls.lock().unwrap().is_empty());
}
#[tokio::test]
async fn writes_follow_official_type_and_packed_boundary_without_automatic_retry() {
    let (_root, _store, api, _session, _service) = fixture();
    let history = HistoryApi::new(api.clone());
    let boundary = HistoryBoundary {
        id: "90071992547409931234".into(),
        played_at: 123,
    };
    history
        .delete(HistoryKind::Song, &["1".into(), "2".into()], &boundary)
        .await
        .unwrap();
    history
        .delete(HistoryKind::Podcast, &["8".into()], &boundary)
        .await
        .unwrap();
    history.clear(HistoryKind::Audiobook).await.unwrap();
    assert_eq!(
        *api.calls.lock().unwrap(),
        vec![
            (
                "/api/play-record/batch/delete".into(),
                json!({"type":"SONG","resourceIds":"[\"1\",\"2\"]","lastPlayTime":123,"lastResourceId":"90071992547409931234"}),
                false
            ),
            (
                "/api/play-record/delete".into(),
                json!({"type":"DJ_RADIO","resourceId":"8","lastPlayTime":123,"lastResourceId":"90071992547409931234"}),
                false
            ),
            (
                "/api/play-record/clear".into(),
                json!({"type":"VOICE_BOOK"}),
                false
            ),
        ]
    );
    assert!(
        history
            .delete(HistoryKind::Song, &["local:123".into()], &boundary)
            .await
            .is_err()
    );
    assert_eq!(api.calls.lock().unwrap().len(), 3);
    api.responses
        .lock()
        .unwrap()
        .insert("/api/play-record/delete".into(), json!({"code":403}));
    assert!(
        history
            .delete(HistoryKind::Podcast, &["8".into()], &boundary)
            .await
            .is_err()
    );
    assert_eq!(api.calls.lock().unwrap().len(), 4);
}

fn delete_two() -> HistoryAction {
    HistoryAction::Delete {
        ids: vec!["2".into()],
    }
}
fn writes(api: &Backend) -> usize {
    api.calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(p, _, _)| p.ends_with("/delete") || p.ends_with("/clear"))
        .count()
}
#[tokio::test]
async fn storage_failure_after_remote_ack_keeps_a_nonreplayable_pending_intent() {
    let (root, store, api, session, service) = fixture();
    let token = session.token().unwrap();
    let page = service
        .refresh_for(&token, HistoryKind::Song)
        .await
        .unwrap();
    let db = rusqlite::Connection::open(root.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_history_ack BEFORE UPDATE ON state WHEN NEW.kind='play-history-mutations-v1' AND json_extract(NEW.value,'$.phase')='confirmed' BEGIN SELECT RAISE(ABORT,'synthetic history commit failure'); END;").unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    assert!(matches!(
        service
            .mutate_for(&token, &id, HistoryKind::Song, &page.revision, delete_two())
            .await,
        Err(Error::Storage(_))
    ));
    assert_eq!(writes(&api), 1);
    assert_eq!(
        store
            .get("99", "play-history-mutations-v1", &id)
            .unwrap()
            .unwrap()["phase"],
        "pending"
    );
    assert_eq!(
        service
            .snapshot_for(&token, HistoryKind::Song)
            .unwrap()
            .remote
            .unwrap()
            .records
            .len(),
        2
    );
    db.execute_batch("DROP TRIGGER fail_history_ack;").unwrap();
    assert_eq!(
        service
            .mutate_for(&token, &id, HistoryKind::Song, &page.revision, delete_two())
            .await
            .unwrap()
            .phase,
        HistoryMutationPhase::Pending
    );
    assert_eq!(writes(&api), 1);
}
#[tokio::test]
async fn persisted_deletion_binds_the_displayed_revision_and_never_replays_an_intent() {
    let (_root, _store, api, session, service) = fixture();
    let token = session.token().unwrap();
    service
        .refresh_for(&token, HistoryKind::Song)
        .await
        .unwrap();
    service
        .record_for(
            &token,
            HistoryKind::Song,
            HistoryRecord::local(HistoryKind::Song, json!({"id":2,"name":"本机"}), 8).unwrap(),
        )
        .unwrap();
    let page = service.snapshot_for(&token, HistoryKind::Song).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    assert!(
        service
            .mutate_for(&token, &id, HistoryKind::Song, "stale", delete_two())
            .await
            .is_err()
    );
    assert!(service.mutations_for(&token).unwrap().is_empty());
    let op = service
        .mutate_for(&token, &id, HistoryKind::Song, &page.revision, delete_two())
        .await
        .unwrap();
    assert_eq!(op.phase, HistoryMutationPhase::Confirmed);
    assert_eq!(op.local_removed, 1);
    assert_eq!(
        op.boundary.as_ref().unwrap(),
        &HistoryBoundary {
            id: "2".into(),
            played_at: 10
        }
    );
    assert_eq!(writes(&api), 1);
    let after = service.snapshot_for(&token, HistoryKind::Song).unwrap();
    assert!(after.local.is_empty());
    assert_eq!(after.entries.len(), 1);
    let same = service
        .mutate_for(&token, &id, HistoryKind::Song, &page.revision, delete_two())
        .await
        .unwrap();
    assert_eq!(same.phase, HistoryMutationPhase::Confirmed);
    assert_eq!(writes(&api), 1);
    assert!(
        service
            .mutate_for(
                &token,
                &id,
                HistoryKind::Album,
                &page.revision,
                HistoryAction::Clear
            )
            .await
            .is_err()
    );
    assert_eq!(writes(&api), 1);
}
#[tokio::test]
async fn lost_write_survives_restart_and_absence_is_not_proof_of_confirmation() {
    let (root, _store, api, session, service) = fixture();
    let token = session.token().unwrap();
    let page = service
        .refresh_for(&token, HistoryKind::Song)
        .await
        .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    api.write_error.store(1, Ordering::SeqCst);
    let op = service
        .mutate_for(&token, &id, HistoryKind::Song, &page.revision, delete_two())
        .await
        .unwrap();
    assert_eq!(op.phase, HistoryMutationPhase::Uncertain);
    assert!(matches!(
        service
            .mutate_for(
                &token,
                &uuid::Uuid::new_v4().to_string(),
                HistoryKind::Song,
                &page.revision,
                HistoryAction::Clear
            )
            .await,
        Err(Error::MutationUncertain)
    ));
    let reopened = Arc::new(AccountSession::new(
        api.clone(),
        Some(Arc::new(StateStore::open(root.path()).unwrap())),
    ));
    reopened
        .adopt(
            api.clone(),
            json!({"userId":99}),
            &reopened.token().unwrap(),
        )
        .unwrap();
    let service = HistoryService::new(reopened.clone()).unwrap();
    let token = reopened.token().unwrap();
    let again = service
        .mutate_for(&token, &id, HistoryKind::Song, &page.revision, delete_two())
        .await
        .unwrap();
    assert_eq!(again.phase, HistoryMutationPhase::Uncertain);
    assert_eq!(writes(&api), 1);
    assert!(
        service
            .accept_current_for(&token, &id, &page.revision)
            .is_err()
    );
    api.responses.lock().unwrap().insert(
        "/api/play-record/song/list".into(),
        json!({"code":200,"data":{"list":[]}}),
    );
    let checked = service.reconcile_for(&token, &id).await.unwrap();
    assert!(checked.last_read_at.is_some());
    assert_eq!(checked.phase, HistoryMutationPhase::Uncertain);
    assert_eq!(writes(&api), 1);
    let current = service.snapshot_for(&token, HistoryKind::Song).unwrap();
    assert!(current.entries.is_empty());
    let accepted = service
        .accept_current_for(&token, &id, &current.revision)
        .unwrap();
    assert_eq!(accepted.phase, HistoryMutationPhase::AcceptedCurrentState);
    assert_eq!(
        service.reconcile_for(&token, &id).await.unwrap().phase,
        HistoryMutationPhase::AcceptedCurrentState
    );
    assert_eq!(writes(&api), 1);
}
#[tokio::test]
async fn native_replays_arriving_while_delete_waits_are_not_deleted_on_acknowledgement() {
    let (_root, _store, api, session, service) = fixture();
    let token = session.token().unwrap();
    service
        .refresh_for(&token, HistoryKind::Song)
        .await
        .unwrap();
    service
        .record_for(
            &token,
            HistoryKind::Song,
            HistoryRecord::local(HistoryKind::Song, json!({"id":2}), 8).unwrap(),
        )
        .unwrap();
    let page = service.snapshot_for(&token, HistoryKind::Song).unwrap();
    api.hold_write.store(true, Ordering::SeqCst);
    let id = uuid::Uuid::new_v4().to_string();
    let task = tokio::spawn({
        let service = service.clone();
        let token = token.clone();
        let id = id.clone();
        async move {
            service
                .mutate_for(&token, &id, HistoryKind::Song, &page.revision, delete_two())
                .await
        }
    });
    api.started.notified().await;
    let journal = service.mutations_for(&token).unwrap();
    assert_eq!(journal[0].phase, HistoryMutationPhase::Pending);
    service
        .record_for(
            &token,
            HistoryKind::Song,
            HistoryRecord::local(
                HistoryKind::Song,
                json!({"id":2,"name":"删除期间的新播放"}),
                100,
            )
            .unwrap(),
        )
        .unwrap();
    api.release.notify_one();
    assert_eq!(
        task.await.unwrap().unwrap().phase,
        HistoryMutationPhase::Confirmed
    );
    let after = service.snapshot_for(&token, HistoryKind::Song).unwrap();
    assert_eq!(after.entries[0].resource["name"], "删除期间的新播放");
    assert_eq!(after.local.len(), 1);
    assert_eq!(writes(&api), 1);
}
#[tokio::test]
async fn late_mutation_result_cannot_publish_across_account_generation() {
    let (_root, store, api, session, service) = fixture();
    let token = session.token().unwrap();
    let page = service
        .refresh_for(&token, HistoryKind::Song)
        .await
        .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    api.hold_write.store(true, Ordering::SeqCst);
    let task = tokio::spawn({
        let service = service.clone();
        let token = token.clone();
        let id = id.clone();
        async move {
            service
                .mutate_for(&token, &id, HistoryKind::Song, &page.revision, delete_two())
                .await
        }
    });
    api.started.notified().await;
    session
        .adopt(api.clone(), json!({"userId":100}), &token)
        .unwrap();
    api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert!(
        service
            .mutations_for(&session.token().unwrap())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .get("99", "play-history-mutations-v1", &id)
            .unwrap()
            .unwrap()["phase"],
        "pending"
    );
    session
        .adopt(api.clone(), json!({"userId":99}), &session.token().unwrap())
        .unwrap();
    let operation = service
        .mutations_for(&session.token().unwrap())
        .unwrap()
        .remove(0);
    let same = service
        .mutate_for(
            &session.token().unwrap(),
            &id,
            HistoryKind::Song,
            &operation.expected_revision,
            delete_two(),
        )
        .await
        .unwrap();
    assert_eq!(same.phase, HistoryMutationPhase::Pending);
    assert_eq!(writes(&api), 1);
}
#[tokio::test]
async fn local_only_clear_and_definite_rejection_preserve_honest_partial_outcomes() {
    let (_root, _store, api, session, service) = fixture();
    let token = session.token().unwrap();
    service.set_sync_for(&token, false).unwrap();
    service
        .record_for(
            &token,
            HistoryKind::Song,
            HistoryRecord::local(HistoryKind::Song, json!({"local_id":"fixture-file"}), 1).unwrap(),
        )
        .unwrap();
    let page = service.snapshot_for(&token, HistoryKind::Song).unwrap();
    let op = service
        .mutate_for(
            &token,
            &uuid::Uuid::new_v4().to_string(),
            HistoryKind::Song,
            &page.revision,
            HistoryAction::Clear,
        )
        .await
        .unwrap();
    assert_eq!(op.phase, HistoryMutationPhase::Confirmed);
    assert!(!op.remote_planned);
    assert_eq!(op.local_removed, 1);
    assert_eq!(writes(&api), 0);
    service.set_sync_for(&token, true).unwrap();
    let page = service
        .refresh_for(&token, HistoryKind::Song)
        .await
        .unwrap();
    api.write_error.store(2, Ordering::SeqCst);
    let id = uuid::Uuid::new_v4().to_string();
    let op = service
        .mutate_for(&token, &id, HistoryKind::Song, &page.revision, delete_two())
        .await
        .unwrap();
    assert_eq!(op.phase, HistoryMutationPhase::Rejected);
    assert_eq!(
        service
            .snapshot_for(&token, HistoryKind::Song)
            .unwrap()
            .entries
            .len(),
        2
    );
    service
        .mutate_for(&token, &id, HistoryKind::Song, &page.revision, delete_two())
        .await
        .unwrap();
    assert_eq!(writes(&api), 1);
}

#[tokio::test]
async fn cancelling_unseen_intent_prevents_late_execution_without_hiding_started_work() {
    let (_root, _store, api, session, service) = fixture();
    let token = session.token().unwrap();
    let page = service
        .refresh_for(&token, HistoryKind::Song)
        .await
        .unwrap();
    let calls = api.calls.lock().unwrap().len();
    let id = uuid::Uuid::new_v4().to_string();
    let op = service
        .cancel_unseen_for(
            &token,
            &id,
            HistoryKind::Song,
            &page.revision,
            HistoryAction::Clear,
        )
        .unwrap();
    assert_eq!(op.phase, HistoryMutationPhase::Cancelled);
    let late = service
        .mutate_for(
            &token,
            &id,
            HistoryKind::Song,
            &page.revision,
            HistoryAction::Clear,
        )
        .await
        .unwrap();
    assert_eq!(late.phase, HistoryMutationPhase::Cancelled);
    assert_eq!(api.calls.lock().unwrap().len(), calls);
    assert_eq!(
        service
            .snapshot_for(&token, HistoryKind::Song)
            .unwrap()
            .entries
            .len(),
        2
    );
    assert!(
        service
            .cancel_unseen_for(
                &token,
                &id,
                HistoryKind::Album,
                &page.revision,
                HistoryAction::Clear
            )
            .is_err()
    );
    let actual_id = uuid::Uuid::new_v4().to_string();
    api.write_error.store(1, Ordering::SeqCst);
    let actual = service
        .mutate_for(
            &token,
            &actual_id,
            HistoryKind::Song,
            &page.revision,
            HistoryAction::Clear,
        )
        .await
        .unwrap();
    assert_eq!(actual.phase, HistoryMutationPhase::Uncertain);
    let actual = service
        .cancel_unseen_for(
            &token,
            &actual_id,
            HistoryKind::Song,
            &page.revision,
            HistoryAction::Clear,
        )
        .unwrap();
    assert_eq!(actual.phase, HistoryMutationPhase::Uncertain);
    assert!(actual.remote_planned);
}
#[tokio::test]
async fn history_revision_ignores_progress_checkpoints_but_fences_new_plays_and_metadata() {
    let (_root, _store, _api, session, service) = fixture();
    let token = session.token().unwrap();
    let kind = HistoryKind::Voice;
    let mut row =
        HistoryRecord::local(kind, json!({"id":7,"name":"声音","mainSong":{"id":77}}), 10).unwrap();
    service.record_for(&token, kind, row.clone()).unwrap();
    let first = service.snapshot_for(&token, kind).unwrap().revision;
    row.play_progress = Some(42.5);
    row.listened = Some(true);
    service.record_for(&token, kind, row.clone()).unwrap();
    let progressed = service.snapshot_for(&token, kind).unwrap();
    assert_eq!(first, progressed.revision);
    assert_eq!(progressed.entries[0].play_progress, Some(42.5));
    row.played_at += 1;
    service.record_for(&token, kind, row.clone()).unwrap();
    let replay = service.snapshot_for(&token, kind).unwrap().revision;
    assert_ne!(first, replay);
    row.resource["name"] = json!("更新名称");
    service.record_for(&token, kind, row).unwrap();
    assert_ne!(replay, service.snapshot_for(&token, kind).unwrap().revision);
}
