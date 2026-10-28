use cumulus_core::{
    Error, Result,
    account::{AccountSession, SessionToken},
    protocol::api::{Api, ApiFuture},
    relay::{
        RelayCommand, RelayConfig, RelayMode, RelayResource, RelayResourceKind, RelaySource,
        RelaySourceKind, RelayUpload, SnapshotItem, publisher::*,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Notify;

#[derive(Clone, Copy)]
enum Reply {
    Ok,
    Code(i64),
    StringCode(i64),
    Lost,
}
struct Backend {
    device: Mutex<Option<String>>,
    enabled: AtomicBool,
    eligible: AtomicBool,
    offline: AtomicBool,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
    replies: Mutex<VecDeque<Reply>>,
    writes: Mutex<Vec<(String, Value)>>,
}
impl Default for Backend {
    fn default() -> Self {
        Self {
            device: Mutex::new(Some("SYNTHETIC-native-device".into())),
            enabled: AtomicBool::new(true),
            eligible: AtomicBool::new(true),
            offline: AtomicBool::new(false),
            hold: AtomicBool::new(false),
            started: Notify::new(),
            release: Notify::new(),
            replies: Mutex::default(),
            writes: Mutex::default(),
        }
    }
}
impl Api for Backend {
    fn local_device_id(&self) -> Result<Option<String>> {
        Ok(self.device.lock().unwrap().clone())
    }
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            if self.offline.load(Ordering::SeqCst) {
                return Err(Error::Transport("synthetic preflight offline".into()));
            }
            match path {
                "/api/rtrs/abt/front/expinfo/list" => Ok(
                    json!({"code":200,"data":[{"expName":"device_connect","expGroupName":if self.eligible.load(Ordering::SeqCst){"t2"}else{"control"}}]}),
                ),
                "/api/relay/config/get" => Ok(
                    json!({"code":200,"data":{"enable":self.enabled.load(Ordering::SeqCst),"snapshotSize":100,"radioProgressInterval":3}}),
                ),
                "/api/relay/songlist/submit" | "/api/relay/play/state/submit" => {
                    let field = if path.ends_with("songlist/submit") {
                        "songListSubmitReq"
                    } else {
                        "playStateSubmitReq"
                    };
                    assert_eq!(payload.as_object().unwrap().len(), 1);
                    let wire = serde_json::from_str(
                        payload[field]
                            .as_str()
                            .expect("official nested JSON string"),
                    )
                    .unwrap();
                    self.writes.lock().unwrap().push((path.into(), wire));
                    let reply = self
                        .replies
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or(Reply::Ok);
                    if self.hold.swap(false, Ordering::SeqCst) {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    match reply {
                        Reply::Ok => Ok(json!({"code":200})),
                        Reply::Code(code) => Ok(json!({"code":code})),
                        Reply::StringCode(code) => Ok(json!({"code":code.to_string()})),
                        Reply::Lost => {
                            Err(Error::Transport("synthetic lost acknowledgement".into()))
                        }
                    }
                }
                _ => Err(Error::InvalidInput("unexpected publication endpoint")),
            }
        })
    }
}
struct World {
    root: tempfile::TempDir,
    store: Arc<StateStore>,
    api: Arc<Backend>,
    session: Arc<AccountSession>,
    publisher: Arc<RelayPublisher>,
}
impl World {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let api = Arc::new(Backend::default());
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
        let publisher = Arc::new(RelayPublisher::new(session.clone()).unwrap());
        Self {
            root,
            store,
            api,
            session,
            publisher,
        }
    }
    fn token(&self) -> SessionToken {
        self.session.token().unwrap()
    }
    fn record(&self, frame: PublicationFrame, event: PublicationEvent) -> Vec<String> {
        self.publisher
            .record_for(&self.token(), frame, event, &RelayConfig::default())
            .unwrap()
    }
    async fn send(&self) -> PublishOutcome {
        self.publisher
            .publish_next_for(&self.token())
            .await
            .unwrap()
    }
    fn replies(&self, replies: &[Reply]) {
        self.api.replies.lock().unwrap().extend(replies);
    }
    fn wire(&self) -> Vec<Value> {
        self.api
            .writes
            .lock()
            .unwrap()
            .iter()
            .map(|(_, v)| v.clone())
            .collect()
    }
    fn rows(&self) -> Vec<PublicationIntent> {
        self.publisher.intents_for(&self.token()).unwrap()
    }
    fn reopen(&self) -> (Arc<AccountSession>, RelayPublisher) {
        let store = Arc::new(StateStore::open(self.root.path()).unwrap());
        let session = Arc::new(AccountSession::new(self.api.clone(), Some(store)));
        session
            .adopt(
                self.api.clone(),
                json!({"userId":9}),
                &session.token().unwrap(),
            )
            .unwrap();
        let publisher = RelayPublisher::new(session.clone()).unwrap();
        (session, publisher)
    }
}
fn song(id: u64) -> RelayResource {
    RelayResource {
        id: id.to_string(),
        kind: RelayResourceKind::Song,
    }
}
fn voice(id: u64) -> RelayResource {
    RelayResource {
        id: id.to_string(),
        kind: RelayResourceKind::Voice,
    }
}
fn frame(revision: u64, resources: Vec<RelayResource>, selected: usize) -> PublicationFrame {
    PublicationFrame {
        queue_revision: revision,
        items: resources
            .iter()
            .enumerate()
            .map(|(n, r)| SnapshotItem {
                entry_id: format!("entry-{n}"),
                resource: r.clone(),
            })
            .collect(),
        selected_entry: resources.get(selected).map(|_| format!("entry-{selected}")),
        current: resources.get(selected).cloned(),
        source: (!resources.is_empty()).then_some(RelaySource::Resources { resources }),
        mode: Some(RelayMode::ListLoop),
        eligible_scene: true,
    }
}
fn value(upload: &RelayUpload) -> &Value {
    match upload {
        RelayUpload::Source { value, .. }
        | RelayUpload::State(value)
        | RelayUpload::Commands(value) => value,
    }
}

#[tokio::test]
async fn durable_official_wire_retains_typed_cumulative_commands_after_success_and_reopen() {
    let w = World::new();
    let mut f = frame(1, vec![song(1), voice(2)], 0);
    f.source = Some(RelaySource::Collection {
        source_type: RelaySourceKind::Playlist,
        id: "90071992547409931234".into(),
    });
    w.record(f.clone(), PublicationEvent::Source);
    assert!(w.wire().is_empty());
    assert!(matches!(w.send().await, PublishOutcome::Confirmed(_)));
    let source = w.wire()[0].clone();
    assert_eq!(source["sourceId"], "90071992547409931234");
    assert_eq!(source["sourceType"], "playlist");
    assert_eq!(source["playMode"], "list_loop");
    assert_eq!(source["retransmit"], false);
    assert_eq!(source["sessionId"].as_str().unwrap().len(), 12);
    assert!(
        w.record(
            f.clone(),
            PublicationEvent::VoiceProgress { progress_ms: 100 }
        )
        .is_empty()
    );
    assert!(
        w.record(f.clone(), PublicationEvent::Mode { from_handoff: true })
            .is_empty()
    );
    let collection = f.source.clone();
    f = frame(2, vec![song(1), voice(2), voice(3)], 0);
    f.source = collection.clone();
    w.record(
        f.clone(),
        PublicationEvent::Command {
            command: RelayCommand::Insert {
                resources: vec![voice(3)],
                pre_id: Some("2".into()),
            },
        },
    );
    w.send().await;
    let (session, publisher) = w.reopen();
    f = frame(3, vec![voice(3)], 0);
    f.source = collection;
    publisher
        .record_for(
            &session.token().unwrap(),
            f.clone(),
            PublicationEvent::Command {
                command: RelayCommand::Delete {
                    resources: vec![song(1), voice(2)],
                },
            },
            &RelayConfig::default(),
        )
        .unwrap();
    publisher
        .publish_next_for(&session.token().unwrap())
        .await
        .unwrap();
    let wire = w.wire();
    assert_eq!(
        wire[2]["commands"],
        json!([{"command":"insert","resources":[{"id":"3","type":"voice"}],"preId":"2"},{"command":"delete","resources":[{"id":"1","type":"song"},{"id":"2","type":"voice"}]}])
    );
    assert_eq!(wire[2]["sessionId"], source["sessionId"]);
    f.queue_revision = 4;
    w.record(f.clone(), PublicationEvent::Select);
    w.send().await;
    w.record(
        f.clone(),
        PublicationEvent::VoiceProgress { progress_ms: 12345 },
    );
    w.send().await;
    assert_eq!(w.wire().last().unwrap()["progress"], 12345);
    assert_eq!(
        w.wire().last().unwrap()["resource"],
        json!({"id":"3","type":"voice"})
    );
    w.record(
        frame(5, vec![], 0),
        PublicationEvent::Command {
            command: RelayCommand::Clear,
        },
    );
    w.send().await;
    assert_eq!(
        w.wire().last().unwrap()["commands"]
            .as_array()
            .unwrap()
            .last()
            .unwrap(),
        &json!({"command":"clear"})
    );
    assert!(
        w.record(frame(5, vec![], 0), PublicationEvent::ColdStart)
            .is_empty()
    );
    assert_eq!(w.publisher.status_for(&w.token()).unwrap().pending, 0);
}

#[tokio::test]
async fn stale_session_recovers_once_from_latest_queue_without_losing_inflight_edits() {
    let w = World::new();
    w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source);
    w.replies(&[Reply::StringCode(10001), Reply::Ok]);
    w.api.hold.store(true, Ordering::SeqCst);
    let publisher = w.publisher.clone();
    let token = w.token();
    let work = tokio::spawn(async move { publisher.publish_next_for(&token).await });
    w.api.started.notified().await;
    // The recorder commits while the HTTP request holds the relay network lock.
    let latest = frame(2, (1..=200).map(song).collect(), 100);
    w.record(latest, PublicationEvent::Source);
    w.api.release.notify_one();
    assert!(matches!(
        work.await.unwrap().unwrap(),
        PublishOutcome::Recovered { .. }
    ));
    let wire = w.wire();
    assert_eq!(wire.len(), 2);
    assert_eq!(wire[1]["initResId"], "101");
    assert_eq!(wire[1]["resources"].as_array().unwrap().len(), 100);
    assert_eq!(wire[1]["resources"][0]["id"], "51");
    assert_eq!(wire[1]["resources"][99]["id"], "150");
    assert_eq!(wire[1]["retransmit"], true);
    assert_ne!(wire[1]["sessionId"], wire[0]["sessionId"]);
    let rows = w.rows();
    assert_eq!(
        rows.iter().map(|r| r.phase).collect::<Vec<_>>(),
        vec![
            PublicationPhase::Rejected,
            PublicationPhase::Superseded,
            PublicationPhase::Confirmed
        ]
    );
    assert_eq!(rows[0].service_code, Some(10001));
    assert_eq!(rows[2].recovery_of.as_deref(), Some(rows[0].id.as_str()));
    assert_eq!(w.send().await, PublishOutcome::Idle);
}

#[tokio::test]
async fn second_10001_blocks_without_repeating_snapshot() {
    let w = World::new();
    w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source);
    w.replies(&[Reply::Code(10001), Reply::Code(10001)]);
    let outcome = w.send().await;
    assert!(matches!(outcome, PublishOutcome::Blocked(_)));
    assert_eq!(w.send().await, outcome);
    assert_eq!(w.wire().len(), 2);
    assert_eq!(
        w.rows()
            .iter()
            .filter(|r| r.service_code == Some(10001))
            .count(),
        2
    );
}

#[tokio::test]
async fn rejection_and_timeout_block_cumulative_replay_until_explicit_idempotent_new_snapshot() {
    for reply in [Reply::Code(403), Reply::Lost] {
        let w = World::new();
        w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source);
        w.send().await;
        let f = frame(2, vec![song(1), voice(2)], 1);
        let blocked = w.record(
            f.clone(),
            PublicationEvent::Command {
                command: RelayCommand::Insert {
                    resources: vec![voice(2)],
                    pre_id: Some("1".into()),
                },
            },
        )[0]
        .clone();
        w.replies(&[reply]);
        assert_eq!(w.send().await, PublishOutcome::Blocked(blocked.clone()));
        w.record(f.clone(), PublicationEvent::Source);
        assert_eq!(w.send().await, PublishOutcome::Blocked(blocked.clone()));
        assert_eq!(w.wire().len(), 2);
        let id = uuid::Uuid::new_v4().to_string();
        let recovery = w
            .publisher
            .resynchronize_for(&w.token(), &id, &blocked)
            .await
            .unwrap();
        assert_eq!(recovery.id, id);
        assert_eq!(recovery.phase, PublicationPhase::Prepared);
        let status = w.publisher.status_for(&w.token()).unwrap();
        assert_eq!(status.blocked.unwrap().id, blocked);
        assert_eq!(status.recovering.unwrap().id, id);
        assert!(!status.can_resynchronize);
        assert_eq!(value(&recovery.upload)["resources"][1]["type"], "voice");
        assert_eq!(
            w.publisher
                .resynchronize_for(&w.token(), &id, &blocked)
                .await
                .unwrap()
                .id,
            id
        );
        assert_eq!(w.send().await, PublishOutcome::Confirmed(id.clone()));
        assert!(
            w.publisher
                .status_for(&w.token())
                .unwrap()
                .blocked
                .is_none()
        );
        assert_eq!(
            w.publisher
                .resynchronize_for(&w.token(), &id, &blocked)
                .await
                .unwrap()
                .phase,
            PublicationPhase::Confirmed
        );
        assert_eq!(w.send().await, PublishOutcome::Idle);
        assert_eq!(w.wire().len(), 3);
        assert!(
            w.publisher
                .resynchronize_for(&w.token(), &id, &uuid::Uuid::new_v4().to_string())
                .await
                .is_err()
        );
        let original = w.rows().into_iter().find(|r| r.id == blocked).unwrap();
        assert_eq!(
            original.phase,
            if matches!(reply, Reply::Lost) {
                PublicationPhase::Uncertain
            } else {
                PublicationPhase::Rejected
            }
        );
    }
}

#[tokio::test]
async fn cancelled_sender_and_process_reopen_retain_unknown_without_automatic_replay() {
    let w = World::new();
    let id = w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source)[0].clone();
    w.api.hold.store(true, Ordering::SeqCst);
    let publisher = w.publisher.clone();
    let token = w.token();
    let work = tokio::spawn(async move { publisher.publish_next_for(&token).await });
    w.api.started.notified().await;
    assert!(matches!(
        w.publisher.publish_next_for(&w.token()).await,
        Err(Error::MutationBusy)
    ));
    work.abort();
    assert!(work.await.unwrap_err().is_cancelled());
    assert_eq!(w.rows()[0].phase, PublicationPhase::Uncertain);
    // Simulate abrupt process death before Drop could persist uncertainty.
    let mut raw = w
        .store
        .get("9", "relay-publication-intents-v1", &id)
        .unwrap()
        .unwrap();
    raw["phase"] = json!("sending");
    w.store
        .put("9", "relay-publication-intents-v1", &id, &raw)
        .unwrap();
    w.store
        .put(
            "9",
            &format!(
                "relay-publication-active-{}",
                hex::encode(Sha256::digest(b"SYNTHETIC-native-device"))
            ),
            &id,
            &raw,
        )
        .unwrap();
    let (session, publisher) = w.reopen();
    assert_eq!(
        publisher
            .publish_next_for(&session.token().unwrap())
            .await
            .unwrap(),
        PublishOutcome::Blocked(id)
    );
    assert_eq!(w.wire().len(), 1);
    assert_eq!(
        publisher.intents_for(&session.token().unwrap()).unwrap()[0].phase,
        PublicationPhase::Uncertain
    );
}

#[tokio::test]
async fn skipped_recovery_keeps_original_unknown_blocked_until_a_new_explicit_snapshot_is_confirmed()
 {
    let w = World::new();
    let blocked = w.record(frame(1, vec![voice(1)], 0), PublicationEvent::Source)[0].clone();
    w.replies(&[Reply::Lost]);
    w.send().await;
    let recovery = uuid::Uuid::new_v4().to_string();
    w.publisher
        .resynchronize_for(&w.token(), &recovery, &blocked)
        .await
        .unwrap();
    assert!(matches!(
        w.publisher
            .resynchronize_for(&w.token(), &uuid::Uuid::new_v4().to_string(), &blocked)
            .await,
        Err(Error::MutationBusy)
    ));
    w.api.enabled.store(false, Ordering::SeqCst);
    assert_eq!(w.send().await, PublishOutcome::Skipped);
    let status = w.publisher.status_for(&w.token()).unwrap();
    assert_eq!(status.blocked.unwrap().id, blocked);
    assert!(status.recovering.is_none());
    assert_eq!(status.latest.unwrap().phase, PublicationPhase::Skipped);
    w.api.enabled.store(true, Ordering::SeqCst);
    w.record(frame(2, vec![song(2)], 0), PublicationEvent::Source);
    assert_eq!(w.send().await, PublishOutcome::Blocked(blocked.clone()));
    assert_eq!(w.wire().len(), 1);
    let fresh = uuid::Uuid::new_v4().to_string();
    w.publisher
        .resynchronize_for(&w.token(), &fresh, &blocked)
        .await
        .unwrap();
    assert_eq!(w.send().await, PublishOutcome::Confirmed(fresh));
    assert_eq!(w.wire().len(), 2);
}

#[tokio::test]
async fn account_change_during_send_records_original_owner_and_hides_receipts_from_new_owner() {
    let w = World::new();
    w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source);
    w.api.hold.store(true, Ordering::SeqCst);
    let publisher = w.publisher.clone();
    let token = w.token();
    let work = tokio::spawn(async move { publisher.publish_next_for(&token).await });
    w.api.started.notified().await;
    w.session
        .adopt(w.api.clone(), json!({"userId":10}), &w.token())
        .unwrap();
    w.api.release.notify_one();
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert!(w.rows().is_empty());
    assert_eq!(w.send().await, PublishOutcome::Idle);
    let old = w.store.values("9", "relay-publication-intents-v1").unwrap();
    assert_eq!(old.len(), 1);
    assert_eq!(old.values().next().unwrap()["phase"], "uncertain");
    w.session
        .adopt(w.api.clone(), json!({"userId":9}), &w.token())
        .unwrap();
    assert!(matches!(w.send().await, PublishOutcome::Blocked(_)));
    assert_eq!(w.wire().len(), 1);
}

#[tokio::test]
async fn native_device_identity_isolated_and_old_recovery_uuid_cannot_rebind() {
    let w = World::new();
    let id = w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source)[0].clone();
    w.replies(&[Reply::Lost]);
    w.send().await;
    let recovery = uuid::Uuid::new_v4().to_string();
    w.publisher
        .resynchronize_for(&w.token(), &recovery, &id)
        .await
        .unwrap();
    *w.api.device.lock().unwrap() = Some("SYNTHETIC-other-device".into());
    assert!(w.rows().is_empty());
    assert_eq!(w.send().await, PublishOutcome::Idle);
    assert!(
        w.publisher
            .resynchronize_for(&w.token(), &recovery, &id)
            .await
            .is_err()
    );
    *w.api.device.lock().unwrap() = None;
    assert!(w.publisher.status_for(&w.token()).is_err());
    assert!(
        w.publisher
            .record_for(
                &w.token(),
                frame(2, vec![song(2)], 0),
                PublicationEvent::Source,
                &RelayConfig::default()
            )
            .is_err()
    );
    assert_eq!(w.wire().len(), 1);
}

#[tokio::test]
async fn device_change_during_ack_keeps_original_receipt_but_does_not_disclose_it_as_new_device_result()
 {
    let w = World::new();
    let id = w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source)[0].clone();
    w.api.hold.store(true, Ordering::SeqCst);
    let publisher = w.publisher.clone();
    let token = w.token();
    let work = tokio::spawn(async move { publisher.publish_next_for(&token).await });
    w.api.started.notified().await;
    *w.api.device.lock().unwrap() = Some("SYNTHETIC-replaced-device".into());
    w.api.release.notify_one();
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert!(w.rows().is_empty());
    assert_eq!(w.send().await, PublishOutcome::Idle);
    let receipt = w
        .store
        .get("9", "relay-publication-intents-v1", &id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt["phase"], "confirmed");
    assert_eq!(receipt["device_id"], "SYNTHETIC-native-device");
    assert_eq!(w.wire().len(), 1);
}

#[tokio::test]
async fn fresh_eligibility_controls_dispatch_and_preflight_failure_never_marks_a_write_sent() {
    let w = World::new();
    w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source);
    w.api.offline.store(true, Ordering::SeqCst);
    assert!(w.publisher.publish_next_for(&w.token()).await.is_err());
    assert_eq!(w.rows()[0].phase, PublicationPhase::Prepared);
    assert!(w.wire().is_empty());
    w.api.offline.store(false, Ordering::SeqCst);
    w.api.eligible.store(false, Ordering::SeqCst);
    assert_eq!(w.send().await, PublishOutcome::Skipped);
    assert_eq!(w.rows()[0].phase, PublicationPhase::Skipped);
    w.api.eligible.store(true, Ordering::SeqCst);
    w.api.enabled.store(false, Ordering::SeqCst);
    w.record(
        frame(2, vec![song(1), song(2)], 1),
        PublicationEvent::Source,
    );
    assert_eq!(w.send().await, PublishOutcome::Skipped);
    assert!(w.wire().is_empty());
    w.api.enabled.store(true, Ordering::SeqCst);
    w.record(
        frame(3, vec![song(1), song(2)], 1),
        PublicationEvent::Mode {
            from_handoff: false,
        },
    );
    w.send().await;
    assert_eq!(w.wire().len(), 1);
    assert_eq!(w.wire()[0]["retransmit"], true);
    assert_eq!(w.wire()[0]["initResId"], "2");
    assert!(w.wire()[0]["commands"].is_null());
}

#[tokio::test]
async fn clear_removes_local_machine_on_failure_and_excluded_queue_never_becomes_snapshot() {
    let w = World::new();
    w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source);
    w.send().await;
    let mut local = frame(2, vec![song(1)], 0);
    local.eligible_scene = false;
    let id = w.record(local.clone(), PublicationEvent::Select)[0].clone();
    w.replies(&[Reply::Code(10001)]);
    assert_eq!(w.send().await, PublishOutcome::Blocked(id.clone()));
    assert_eq!(w.wire().len(), 2);
    assert!(w.record(local, PublicationEvent::ColdStart).is_empty());
    assert!(
        !w.publisher
            .status_for(&w.token())
            .unwrap()
            .can_resynchronize
    );
    assert!(
        w.publisher
            .resynchronize_for(&w.token(), &uuid::Uuid::new_v4().to_string(), &id)
            .await
            .is_err()
    );
    assert_eq!(w.wire().len(), 2);
    w.record(frame(3, vec![voice(9)], 0), PublicationEvent::Source);
    let fresh = uuid::Uuid::new_v4().to_string();
    w.publisher
        .resynchronize_for(&w.token(), &fresh, &id)
        .await
        .unwrap();
    assert_eq!(w.send().await, PublishOutcome::Confirmed(fresh));
}

#[tokio::test]
async fn backlog_compaction_preserves_unknown_responsibility_and_rebases_only_unsent_queue() {
    let w = World::new();
    let blocked = w.record(frame(1, vec![voice(1)], 0), PublicationEvent::Source)[0].clone();
    w.replies(&[Reply::Lost]);
    w.send().await;
    for revision in 2..=259 {
        w.record(
            frame(revision, vec![voice(1), song(revision)], 0),
            PublicationEvent::Source,
        );
    }
    let status = w.publisher.status_for(&w.token()).unwrap();
    assert_eq!(status.blocked.unwrap().id, blocked);
    assert!(status.pending <= 2);
    assert_eq!(w.wire().len(), 1);
    assert_eq!(
        w.rows()
            .iter()
            .filter(|r| r.phase == PublicationPhase::Superseded)
            .count(),
        257
    );
    assert_eq!(w.send().await, PublishOutcome::Blocked(blocked));
}

#[test]
fn invalid_or_stale_frame_rolls_back_without_recording_and_terminal_clear_handles_full_machine() {
    let w = World::new();
    w.record(frame(5, vec![song(1)], 0), PublicationEvent::Source);
    assert!(matches!(
        w.publisher.record_for(
            &w.token(),
            frame(4, vec![song(2)], 0),
            PublicationEvent::Source,
            &RelayConfig::default()
        ),
        Err(Error::StaleResponse)
    ));
    let mut invalid = frame(6, vec![song(1), song(2)], 0);
    invalid.items[1].entry_id = invalid.items[0].entry_id.clone();
    assert!(
        w.publisher
            .record_for(
                &w.token(),
                invalid,
                PublicationEvent::Source,
                &RelayConfig::default()
            )
            .is_err()
    );
    let config = RelayConfig {
        snapshot_size: 0,
        ..RelayConfig::default()
    };
    assert!(
        w.publisher
            .record_for(
                &w.token(),
                frame(6, vec![song(2)], 0),
                PublicationEvent::Source,
                &config
            )
            .is_err()
    );
    assert_eq!(w.rows().len(), 1);
    let mut machine = cumulus_core::relay::RelayMachine::start(
        RelaySource::Resources {
            resources: vec![song(1)],
        },
        song(1),
    )
    .unwrap();
    for _ in 0..512 {
        machine
            .command(RelayCommand::Delete {
                resources: vec![song(2)],
            })
            .unwrap();
    }
    let clear = machine.terminal_clear().unwrap();
    let commands = value(&clear)["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 513);
    assert_eq!(commands.last().unwrap(), &json!({"command":"clear"}));
}

#[tokio::test]
async fn backlog_coalescing_cannot_supersede_an_explicit_pending_recovery() {
    let w = World::new();
    let blocked = w.record(frame(1, vec![song(1)], 0), PublicationEvent::Source)[0].clone();
    w.replies(&[Reply::Lost]);
    w.send().await;
    let recovery = uuid::Uuid::new_v4().to_string();
    w.publisher
        .resynchronize_for(&w.token(), &recovery, &blocked)
        .await
        .unwrap();
    for revision in 2..=258 {
        w.record(
            frame(revision, vec![song(revision)], 0),
            PublicationEvent::Source,
        );
    }
    let status = w.publisher.status_for(&w.token()).unwrap();
    assert_eq!(status.blocked.unwrap().id, blocked);
    assert_eq!(status.recovering.unwrap().id, recovery);
    assert_eq!(
        w.rows().iter().find(|r| r.id == recovery).unwrap().phase,
        PublicationPhase::Prepared
    );
    assert_eq!(w.send().await, PublishOutcome::Confirmed(recovery));
    assert!(
        w.publisher
            .status_for(&w.token())
            .unwrap()
            .blocked
            .is_none()
    );
    while w.publisher.status_for(&w.token()).unwrap().pending > 0 {
        assert!(matches!(w.send().await, PublishOutcome::Confirmed(_)));
    }
    assert_eq!(w.wire().last().unwrap()["initResId"], "258");
    let active = format!(
        "relay-publication-active-{}",
        hex::encode(Sha256::digest(b"SYNTHETIC-native-device"))
    );
    assert!(w.store.values("9", &active).unwrap().is_empty());
    assert!(w.rows().len() > 256); // History is retained, but not in the hot working set.
}
