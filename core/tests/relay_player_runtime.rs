#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod audio;
use cumulus_core::{
    Error, Result,
    account::{AccountSession, SessionToken},
    playback::{
        PlaybackMode, PlaybackQueue,
        controller::{
            PlaybackCommand as Cmd, PlaybackController, PlaybackStatus as Status, QueueTransport,
            QueueUpdate,
        },
        engine::EndReason,
        source::SourceResolver,
    },
    protocol::api::{Api, ApiFuture},
    relay::publisher::{PublicationPhase, RelayPublisher},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

#[derive(Default)]
struct Backend {
    audio: audio::Backend,
    writes: Mutex<Vec<Value>>,
    hold: AtomicBool,
    code: AtomicI64,
    offline: AtomicBool,
    started: Notify,
    release: Notify,
}
impl Api for Backend {
    fn local_device_id(&self) -> Result<Option<String>> {
        Ok(Some("SYNTHETIC-native-publisher".into()))
    }
    fn request<'a>(&'a self, path: &'a str, p: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            match path {
                "/api/rtrs/abt/front/expinfo/list" => {
                    if self.offline.load(Ordering::SeqCst) {
                        return Err(Error::Transport("synthetic unavailable".into()));
                    }
                    Ok(
                        json!({"code":200,"data":[{"expName":"device_connect","expGroupName":"t2"}]}),
                    )
                }
                "/api/relay/config/get" => Ok(
                    json!({"code":200,"data":{"enable":true,"radioProgressInterval":3,"snapshotSize":100}}),
                ),
                "/api/relay/songlist/submit" | "/api/relay/play/state/submit" => {
                    let field = if path.ends_with("songlist/submit") {
                        "songListSubmitReq"
                    } else {
                        "playStateSubmitReq"
                    };
                    self.writes
                        .lock()
                        .unwrap()
                        .push(serde_json::from_str(p[field].as_str().unwrap()).unwrap());
                    if self.hold.swap(false, Ordering::SeqCst) {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    let code = self.code.load(Ordering::SeqCst);
                    if code == -1 {
                        return Err(Error::Transport("synthetic lost ack".into()));
                    }
                    Ok(json!({"code":if code==0{200}else{code}}))
                }
                "/api/dj/play/url/check" => Ok(
                    json!({"code":200,"data":{"voiceId":p["voiceId"],"songId":"1005","canPlay":true}}),
                ),
                "/api/dj/program/detail" => Ok(
                    json!({"code":200,"program":{"id":p["id"],"name":"Synthetic voice","mainSong":{"id":1005,"duration":120000},"duration":120000,"radio":{"id":8}}}),
                ),
                _ => self.audio.request(path, p, cache).await,
            }
        })
    }
}
struct World {
    root: tempfile::TempDir,
    store: Arc<StateStore>,
    api: Arc<Backend>,
    session: Arc<AccountSession>,
    player: Arc<PlaybackController>,
    publisher: RelayPublisher,
    device: Arc<audio::Device>,
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
        let device = Arc::new(audio::Device::default());
        let player = Arc::new(
            PlaybackController::start_with_relay_publication(
                session.clone(),
                store.clone(),
                Arc::new(SourceResolver::new(session.clone(), None)),
                audio::audio_engine(device.clone()),
            )
            .unwrap(),
        );
        let publisher = RelayPublisher::new(session.clone()).unwrap();
        Self {
            root,
            store,
            api,
            session,
            player,
            publisher,
            device,
        }
    }
    fn token(&self) -> SessionToken {
        self.session.token().unwrap()
    }
    async fn cmd(&self, cmd: Cmd) -> cumulus_core::playback::controller::PlaybackSnapshot {
        tokio::time::timeout(
            Duration::from_secs(5),
            self.player.command(&self.token(), cmd),
        )
        .await
        .unwrap()
        .unwrap()
    }
    fn wire(&self) -> Vec<Value> {
        self.api.writes.lock().unwrap().clone()
    }
    async fn drained(&self) {
        wait(|| {
            let s = self.publisher.status_for(&self.token()).unwrap();
            s.pending == 0
                && !self
                    .player
                    .relay_runtime()
                    .unwrap()
                    .status()
                    .unwrap()
                    .sending
        })
        .await;
    }
    fn sql(&self, sql: &str) {
        rusqlite::Connection::open(self.root.path().join("core.sqlite3"))
            .unwrap()
            .execute_batch(sql)
            .unwrap();
    }
    fn reopened_player(&self) -> PlaybackController {
        PlaybackController::start_with_relay_publication(
            self.session.clone(),
            self.store.clone(),
            Arc::new(SourceResolver::new(self.session.clone(), None)),
            audio::audio_engine(Arc::new(audio::Device::default())),
        )
        .unwrap()
    }
}
async fn wait(test: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !test() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("relay runtime did not converge");
}
fn source(ids: &[u64]) -> Cmd {
    Cmd::Replace {
        resources: ids.iter().map(|id| json!({"id":id,"dt":120000})).collect(),
        index: 0,
        source: json!({"kind":"playlist","id":77}),
    }
}
fn last_commands(w: &World) -> Vec<Value> {
    w.wire()
        .iter()
        .rev()
        .find_map(|v| v["commands"].as_array().cloned())
        .unwrap()
}

#[tokio::test]
async fn committed_player_commands_and_automatic_eof_reach_official_publication() {
    let w = World::new();
    w.cmd(source(&[1, 2, 3])).await;
    wait(|| w.wire().len() >= 2).await;
    w.drained().await;
    assert_eq!(w.wire()[0]["sourceType"], "playlist");
    assert_eq!(w.wire()[0]["sourceId"], "77");
    w.cmd(Cmd::Append {
        resources: vec![json!({"id":4,"dt":120000})],
        source: json!({"kind":"songs"}),
    })
    .await;
    wait(|| {
        w.wire()
            .iter()
            .any(|v| v["commands"][0]["command"] == "insert")
    })
    .await;
    assert_eq!(last_commands(&w)[0]["preId"], "3");
    let q = w.player.queue(&w.token()).await.unwrap();
    w.cmd(Cmd::Reorder(vec![
        q.items[3].entry_id.clone(),
        q.items[0].entry_id.clone(),
        q.items[1].entry_id.clone(),
        q.items[2].entry_id.clone(),
    ]))
    .await;
    wait(|| last_commands(&w).len() == 2).await;
    assert_eq!(last_commands(&w)[1]["command"], "move");
    assert_eq!(last_commands(&w)[1]["resources"][0]["id"], "4");
    assert!(last_commands(&w)[1]["preId"].is_null());
    w.cmd(Cmd::Remove(vec![
        q.items[1].entry_id.clone(),
        q.items[2].entry_id.clone(),
    ]))
    .await;
    wait(|| last_commands(&w).len() == 3).await;
    assert_eq!(
        last_commands(&w)[2]["resources"],
        json!([{"id":"2","type":"song"},{"id":"3","type":"song"}])
    );
    w.cmd(Cmd::Mode(PlaybackMode::SingleLoop)).await;
    wait(|| {
        w.wire()
            .iter()
            .any(|v| v["playMode"] == "single_loop" && v["resource"].is_object())
    })
    .await;
    w.cmd(Cmd::Mode(PlaybackMode::OrderLoop)).await;
    let native = w.device.state.lock().unwrap().view.entry_id.unwrap();
    audio::event(&w.device, native, EndReason::Eof);
    audio::until(&w.player, |s| {
        s.state == Status::Playing && audio::song(s) == Some(4)
    })
    .await;
    wait(|| w.wire().iter().any(|v| v["resource"]["id"] == "4")).await;
    w.cmd(source(&[])).await;
    wait(|| {
        w.wire().iter().any(|v| {
            v["commands"]
                .as_array()
                .is_some_and(|c| c.last().is_some_and(|c| c["command"] == "clear"))
        })
    })
    .await;
    assert!(
        !w.player
            .relay_runtime()
            .unwrap()
            .status()
            .unwrap()
            .record_failed
    );
}

#[tokio::test]
async fn network_wait_and_rejection_do_not_block_pause_or_restart_stopped_audio() {
    let w = World::new();
    w.api.hold.store(true, Ordering::SeqCst);
    w.cmd(source(&[1, 2])).await;
    w.api.started.notified().await;
    let paused = tokio::time::timeout(
        Duration::from_secs(2),
        w.player.command(&w.token(), Cmd::Pause(true)),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(paused.state, Status::Paused);
    w.cmd(Cmd::Append {
        resources: vec![json!({"id":3})],
        source: json!({"kind":"songs"}),
    })
    .await;
    w.cmd(Cmd::Stop).await;
    w.api.code.store(403, Ordering::SeqCst);
    w.api.release.notify_one();
    wait(|| {
        w.publisher
            .status_for(&w.token())
            .unwrap()
            .blocked
            .is_some()
    })
    .await;
    let count = w.wire().len();
    w.cmd(Cmd::Mode(PlaybackMode::Random)).await;
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(w.wire().len(), count);
    let blocked = w
        .publisher
        .status_for(&w.token())
        .unwrap()
        .blocked
        .unwrap()
        .id;
    w.api.code.store(0, Ordering::SeqCst);
    let id = uuid::Uuid::new_v4().to_string();
    w.publisher
        .resynchronize_for(&w.token(), &id, &blocked)
        .await
        .unwrap();
    w.player.relay_runtime().unwrap().wake();
    wait(|| {
        w.publisher
            .intents_for(&w.token())
            .unwrap()
            .iter()
            .any(|r| r.id == id && r.phase == PublicationPhase::Confirmed)
    })
    .await;
    assert_eq!(w.player.snapshot().state, Status::Idle);
    assert_eq!(
        w.wire().last().unwrap()["resources"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[tokio::test]
async fn voice_timer_stays_throttled_across_its_own_requests_and_paused_seek_is_explicit() {
    let w = World::new();
    w.cmd(Cmd::Replace {
        resources: vec![json!({"id":1005,"voice_id":"5","dt":120000})],
        index: 0,
        source: json!({"kind":"podcast","scope":{"kind":"radio","id":"8","ascending":false}}),
    })
    .await;
    wait(|| w.wire().len() >= 2).await;
    w.drained().await;
    w.cmd(Cmd::Pause(true)).await;
    w.cmd(Cmd::Seek(32.5)).await;
    wait(|| w.wire().iter().any(|v| v["progress"] == 32500)).await;
    w.drained().await;
    let count = w.wire().len();
    tokio::time::sleep(Duration::from_millis(3400)).await;
    assert_eq!(
        w.wire().len(),
        count,
        "paused timer must not submit progress"
    );
    w.cmd(Cmd::Pause(false)).await;
    w.device.state.lock().unwrap().view.position = Some(34.0);
    wait(|| w.wire().iter().any(|v| v["progress"] == 34000)).await;
    w.drained().await;
    let count = w.wire().len();
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert_eq!(
        w.wire().len(),
        count,
        "preflight must not reset the throttle after each successful upload"
    );
    w.cmd(Cmd::Stop).await;
}

#[tokio::test]
async fn reopening_uses_durable_frame_revision_and_external_queue_edits_are_observed() {
    let w = World::new();
    w.cmd(source(&[1, 2])).await;
    wait(|| w.wire().len() >= 2).await;
    w.drained().await;
    w.cmd(Cmd::Stop).await;
    let reopened = w.reopened_player();
    let count = w.wire().len();
    reopened.command(&w.token(), Cmd::Restore).await.unwrap();
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(
        w.wire().len(),
        count,
        "paused restore is not a cold playing event"
    );
    reopened.command(&w.token(), Cmd::Play).await.unwrap();
    wait(|| w.wire().len() > count).await;
    assert!(
        !reopened
            .relay_runtime()
            .unwrap()
            .status()
            .unwrap()
            .record_failed
    );
    let mut external = PlaybackQueue::new(Some(w.store.clone()), "9").unwrap();
    external
        .append(vec![json!({"id":3})], json!({"kind":"songs"}))
        .unwrap();
    wait(|| {
        w.wire().iter().any(|v| {
            v["commands"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|c| c["resources"][0]["id"] == "3"))
        })
    })
    .await;
    wait(|| w.player.snapshot().queue_len == 3 && reopened.snapshot().queue_len == 3).await;
    w.drained().await;
    wait(|| !reopened.relay_runtime().unwrap().status().unwrap().sending).await;
    let commands = last_commands(&w);
    assert_eq!(
        commands
            .iter()
            .filter(|c| c["command"] == "insert" && c["resources"][0]["id"] == "3")
            .count(),
        1,
        "both observers must publish one increment for the same committed queue"
    );
    reopened.command(&w.token(), Cmd::Stop).await.unwrap();
}

#[tokio::test]
async fn account_switch_cancels_inflight_sender_and_never_moves_old_receipt() {
    let w = World::new();
    w.api.hold.store(true, Ordering::SeqCst);
    w.cmd(source(&[1, 2])).await;
    w.api.started.notified().await;
    w.session
        .adopt(w.api.clone(), json!({"userId":10}), &w.token())
        .unwrap();
    wait(|| {
        w.store
            .values("9", "relay-publication-intents-v1")
            .unwrap()
            .values()
            .any(|r| r["phase"] == "uncertain")
    })
    .await;
    w.cmd(source(&[7, 8])).await;
    wait(|| w.wire().iter().any(|v| v["initResId"] == "7")).await;
    w.drained().await;
    assert!(
        w.publisher
            .intents_for(&w.token())
            .unwrap()
            .iter()
            .all(|r| r.phase != PublicationPhase::Uncertain)
    );
    assert_eq!(
        w.player
            .relay_runtime()
            .unwrap()
            .status()
            .unwrap()
            .account
            .uid
            .as_deref(),
        Some("10")
    );
    w.cmd(Cmd::Stop).await;
}

#[tokio::test]
async fn local_record_failure_does_not_fail_audio_and_repairs_from_latest_queue_without_replay() {
    let w = World::new();
    w.cmd(source(&[1, 2])).await;
    wait(|| w.wire().len() >= 2).await;
    w.drained().await;
    w.sql("CREATE TRIGGER fail_relay BEFORE INSERT ON state WHEN NEW.kind='relay-publication-v1' BEGIN SELECT RAISE(ABORT,'synthetic publication disk failure'); END;");
    let count = w.wire().len();
    w.cmd(Cmd::Append {
        resources: vec![json!({"id":3})],
        source: json!({"kind":"songs"}),
    })
    .await;
    wait(|| {
        w.player
            .relay_runtime()
            .unwrap()
            .status()
            .unwrap()
            .record_failed
    })
    .await;
    assert_eq!(w.player.snapshot().state, Status::Playing);
    assert!(w.player.snapshot().error.is_none());
    assert_eq!(w.wire().len(), count);
    w.sql("DROP TRIGGER fail_relay;");
    w.cmd(Cmd::Append {
        resources: vec![json!({"id":4})],
        source: json!({"kind":"songs"}),
    })
    .await;
    wait(|| {
        !w.player
            .relay_runtime()
            .unwrap()
            .status()
            .unwrap()
            .record_failed
    })
    .await;
    wait(|| w.wire().len() > count).await;
    w.drained().await;
    let wire = w.wire();
    let snapshot = wire.iter().rev().find(|v| v["retransmit"] == true).unwrap();
    assert_eq!(snapshot["resources"].as_array().unwrap().len(), 4);
    assert!(snapshot["commands"].is_null());
    w.cmd(Cmd::Stop).await;
}

#[tokio::test]
async fn handoff_mode_suppression_is_one_transition_and_excluded_queues_clear_the_session() {
    let w = World::new();
    w.cmd(source(&[1, 2])).await;
    wait(|| w.wire().len() >= 2).await;
    w.drained().await;
    let before = w.player.snapshot();
    let mut q = w.player.queue(&w.token()).await.unwrap();
    q.mode = PlaybackMode::Random;
    for row in &mut q.items {
        row.source["scene"] = json!("playlistHandoff");
    }
    let count = w.wire().len();
    w.cmd(Cmd::UpdateQueue(QueueUpdate {
        expected: before.fence(),
        queue: q,
        transport: None,
    }))
    .await;
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(w.wire().len(), count);
    w.cmd(Cmd::Mode(PlaybackMode::SingleLoop)).await;
    wait(|| {
        w.wire()
            .iter()
            .any(|v| v["playMode"] == "single_loop" && v["resource"].is_object())
    })
    .await;
    let before = w.player.snapshot();
    let mut q = w.player.queue(&w.token()).await.unwrap();
    for row in &mut q.items {
        row.entry_id = uuid::Uuid::new_v4().to_string();
        row.source = json!({"kind":"cloud"});
    }
    q.order = q.items.iter().map(|i| i.entry_id.clone()).collect();
    q.current = q.order.first().cloned();
    w.cmd(Cmd::UpdateQueue(QueueUpdate {
        expected: before.fence(),
        queue: q,
        transport: Some(QueueTransport {
            position: None,
            paused: true,
        }),
    }))
    .await;
    wait(|| {
        w.wire().iter().any(|v| {
            v["commands"]
                .as_array()
                .is_some_and(|c| c.last().is_some_and(|c| c["command"] == "clear"))
        })
    })
    .await;
    w.drained().await;
    let count = w.wire().len();
    w.cmd(Cmd::Mode(PlaybackMode::OrderLoop)).await;
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(w.wire().len(), count);
    w.cmd(Cmd::Stop).await;
}

#[tokio::test]
async fn recording_failure_cancels_inflight_http_and_disk_repair_retains_its_unknown_receipt() {
    let w = World::new();
    w.api.hold.store(true, Ordering::SeqCst);
    w.cmd(source(&[1, 2])).await;
    w.api.started.notified().await;
    w.sql("CREATE TRIGGER fail_relay BEFORE INSERT ON state WHEN NEW.kind='relay-publication-v1' BEGIN SELECT RAISE(ABORT,'synthetic publication disk failure'); END;");
    w.cmd(Cmd::Append {
        resources: vec![json!({"id":3})],
        source: json!({"kind":"songs"}),
    })
    .await;
    wait(|| {
        w.player
            .relay_runtime()
            .unwrap()
            .status()
            .unwrap()
            .record_failed
    })
    .await;
    // Drop cannot commit its receipt while the disk trigger fails. The journal
    // must still recover Sending as unknown after local recording is repaired.
    w.sql("DROP TRIGGER fail_relay;");
    wait(|| {
        !w.player
            .relay_runtime()
            .unwrap()
            .status()
            .unwrap()
            .record_failed
    })
    .await;
    wait(|| {
        w.publisher
            .status_for(&w.token())
            .unwrap()
            .blocked
            .is_some()
    })
    .await;
    assert_eq!(
        w.publisher
            .status_for(&w.token())
            .unwrap()
            .blocked
            .unwrap()
            .phase,
        PublicationPhase::Uncertain
    );
    assert_eq!(w.wire().len(), 1);
    assert_eq!(w.player.snapshot().state, Status::Playing);
    w.cmd(Cmd::Stop).await;
}
