#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod audio;
use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
    relay::{
        RelayConfig, RelayMode, RelayResource, RelayResourceKind, RelaySource, SnapshotItem,
        publisher::*,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicI64, Ordering},
};
use tokio::sync::Notify;
#[derive(Default)]
struct Backend {
    audio: audio::Backend,
    calls: Mutex<Vec<String>>,
    writes: Mutex<Vec<Value>>,
    code: AtomicI64,
    offline: AtomicBool,
    hold: AtomicBool,
    entered: Notify,
    release: Notify,
}
impl Api for Backend {
    fn local_device_id(&self) -> Result<Option<String>> {
        Ok(Some("SYNTHETIC_PRIVATE-publication".into()))
    }
    fn request<'a>(&'a self, path: &'a str, p: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(path.into());
            if self.offline.load(Ordering::SeqCst) {
                return Err(Error::Transport("synthetic offline".into()));
            }
            Ok(match path {
                "/api/rtrs/abt/front/expinfo/list" => {
                    json!({"code":200,"data":[{"expName":"device_connect","expGroupName":"t2"}]})
                }
                "/api/relay/config/get" => {
                    json!({"code":200,"data":{"enable":true,"snapshotSize":100,"radioProgressInterval":3}})
                }
                "/api/relay/songlist/submit" | "/api/relay/play/state/submit" => {
                    let key = if path.ends_with("songlist/submit") {
                        "songListSubmitReq"
                    } else {
                        "playStateSubmitReq"
                    };
                    self.writes
                        .lock()
                        .unwrap()
                        .push(serde_json::from_str(p[key].as_str().unwrap()).unwrap());
                    if self.hold.swap(false, Ordering::SeqCst) {
                        self.entered.notify_one();
                        self.release.notified().await;
                    }
                    let code = self.code.load(Ordering::SeqCst);
                    json!({"code":if code==0{200}else{code}})
                }
                "/api/song/enhance/player/url/v1" => self.audio.request(path, p, false).await?,
                _ => panic!("unexpected publication request {path}"),
            })
        })
    }
}
struct World {
    _root: tempfile::TempDir,
    session: Arc<AccountSession>,
    api: Arc<Backend>,
    client: MusicClient,
    publisher: Arc<RelayPublisher>,
}
fn frame(revision: u64) -> PublicationFrame {
    let resources = (1..=revision + 1)
        .map(|id| RelayResource {
            id: id.to_string(),
            kind: RelayResourceKind::Song,
        })
        .collect::<Vec<_>>();
    PublicationFrame {
        queue_revision: revision,
        items: resources
            .iter()
            .map(|r| SnapshotItem {
                entry_id: format!("entry-{}", r.id),
                resource: r.clone(),
            })
            .collect(),
        selected_entry: Some("entry-1".into()),
        current: Some(resources[0].clone()),
        source: Some(RelaySource::Resources { resources }),
        mode: Some(RelayMode::ListLoop),
        eligible_scene: true,
    }
}
impl World {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let api = Arc::new(Backend::default());
        let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
        session
            .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
        let publisher = Arc::new(RelayPublisher::new(session.clone()).unwrap());
        let client = MusicClient::with_session(session.clone(), registry)
            .with_relay_publication()
            .with_audio_factory(|| panic!("publication must not initialize audio"));
        Self {
            _root: root,
            session,
            api,
            client,
            publisher,
        }
    }
    fn epoch(&self) -> String {
        self.session.token().unwrap().generation.to_string()
    }
    fn read(&self, id: Option<&str>) -> RelayPublicationView {
        self.client
            .relay_publication_view(&self.epoch(), id)
            .unwrap()
            .value
    }
    async fn blocked(&self) -> String {
        self.api.code.store(403, Ordering::SeqCst);
        let token = self.session.token().unwrap();
        self.publisher
            .record_for(
                &token,
                frame(1),
                PublicationEvent::Source,
                &RelayConfig::default(),
            )
            .unwrap();
        self.publisher.publish_next_for(&token).await.unwrap();
        self.read(None).blocked.unwrap().id
    }
    async fn resync(&self, id: &str, blocked: &str) -> Result<AccountData<RelayPublicationView>> {
        self.client
            .relay_publication_command(
                &self.epoch(),
                RelayPublicationCommand::Resynchronize {
                    id: id.into(),
                    blocked_id: blocked.into(),
                },
            )
            .await
    }
}
#[tokio::test]
async fn status_is_local_whitelisted_and_account_fenced() {
    let w = World::new();
    assert_eq!(w.read(None).pending, 0);
    assert!(w.api.calls.lock().unwrap().is_empty());
    let blocked = w.blocked().await;
    let count = w.api.calls.lock().unwrap().len();
    let view = w.read(Some(&blocked));
    assert!(view.can_resynchronize);
    assert_eq!(view.blocked.unwrap().service_code, Some(403));
    let raw = serde_json::to_string(&w.read(Some(&blocked))).unwrap();
    for private in [
        "PRIVATE",
        "device",
        "sessionId",
        "resources",
        "url",
        "cookie",
    ] {
        assert!(!raw.contains(private));
    }
    assert_eq!(w.api.calls.lock().unwrap().len(), count);
    let old = w.epoch();
    w.session
        .adopt(
            w.api.clone(),
            json!({"userId":10}),
            &w.session.token().unwrap(),
        )
        .unwrap();
    assert!(w.client.relay_publication_view(&old, None).is_err());
    assert!(w.read(None).blocked.is_none());
    assert!(w.read(Some(&blocked)).requested.is_none());
}
#[tokio::test]
async fn explicit_recovery_uses_current_frame_and_duplicate_receipt_never_resends() {
    let w = World::new();
    let blocked = w.blocked().await;
    let token = w.session.token().unwrap();
    w.publisher
        .record_for(
            &token,
            frame(2),
            PublicationEvent::Snapshot,
            &RelayConfig::default(),
        )
        .unwrap();
    w.api.code.store(0, Ordering::SeqCst);
    let id = uuid::Uuid::new_v4().to_string();
    let view = w.resync(&id, &blocked).await.unwrap().value;
    assert!(view.blocked.is_none());
    assert_eq!(view.requested.unwrap().phase, "confirmed");
    let writes = w.api.writes.lock().unwrap().clone();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[1]["resources"].as_array().unwrap().len(), 3);
    assert_eq!(writes[1]["retransmit"], true);
    w.resync(&id, &blocked).await.unwrap();
    assert_eq!(w.api.writes.lock().unwrap().len(), 2);
    assert_eq!(w.read(Some(&blocked)).requested.unwrap().phase, "rejected");
}
#[tokio::test]
async fn unknown_ipc_is_cancelled_durably_and_late_command_cannot_dispatch() {
    let w = World::new();
    let blocked = w.blocked().await;
    let id = uuid::Uuid::new_v4().to_string();
    let command = || RelayPublicationCommand::CancelUnseen {
        id: id.clone(),
        blocked_id: blocked.clone(),
    };
    let view = w
        .client
        .relay_publication_command(&w.epoch(), command())
        .await
        .unwrap()
        .value;
    assert_eq!(view.requested.unwrap().phase, "cancelled");
    assert!(view.can_resynchronize);
    assert!(matches!(
        w.resync(&id, &blocked).await,
        Err(Error::Cancelled)
    ));
    assert_eq!(w.api.writes.lock().unwrap().len(), 1);
    let registry = ProfileRegistry::open(w._root.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(w.api.clone(), Some(registry.store())));
    session
        .adopt(
            w.api.clone(),
            json!({"userId":9}),
            &session.token().unwrap(),
        )
        .unwrap();
    let reopened = MusicClient::with_session(session.clone(), registry);
    assert_eq!(
        reopened
            .relay_publication_view(&session.token().unwrap().generation.to_string(), Some(&id))
            .unwrap()
            .value
            .requested
            .unwrap()
            .phase,
        "cancelled"
    );
    w.client
        .relay_publication_command(&w.epoch(), command())
        .await
        .unwrap();
    assert!(
        w.client
            .relay_publication_command(
                &w.epoch(),
                RelayPublicationCommand::CancelUnseen {
                    id,
                    blocked_id: uuid::Uuid::new_v4().to_string()
                }
            )
            .await
            .is_err()
    );
}
#[tokio::test]
async fn inspection_does_not_recover_live_send_and_cancel_cannot_race_it() {
    let w = World::new();
    let blocked = w.blocked().await;
    let id = uuid::Uuid::new_v4().to_string();
    w.api.code.store(0, Ordering::SeqCst);
    w.api.hold.store(true, Ordering::SeqCst);
    let token = w.session.token().unwrap();
    w.publisher
        .resynchronize_for(&token, &id, &blocked)
        .await
        .unwrap();
    let publisher = w.publisher.clone();
    let send = tokio::spawn(async move { publisher.publish_next_for(&token).await });
    w.api.entered.notified().await;
    assert_eq!(w.read(Some(&id)).requested.unwrap().phase, "sending");
    assert!(matches!(
        w.publisher
            .cancel_unseen_for(&w.session.token().unwrap(), &id, &blocked),
        Err(Error::MutationBusy)
    ));
    send.abort();
    assert!(send.await.unwrap_err().is_cancelled());
    assert_eq!(w.read(Some(&id)).requested.unwrap().phase, "uncertain");
    assert_eq!(w.api.writes.lock().unwrap().len(), 2);
}
#[tokio::test]
async fn prepared_recovery_can_resume_after_preflight_failure_without_replaying_sent_work() {
    let w = World::new();
    let blocked = w.blocked().await;
    let id = uuid::Uuid::new_v4().to_string();
    w.publisher
        .resynchronize_for(&w.session.token().unwrap(), &id, &blocked)
        .await
        .unwrap();
    w.api.offline.store(true, Ordering::SeqCst);
    assert!(w.resync(&id, &blocked).await.is_err());
    assert_eq!(w.read(Some(&id)).requested.unwrap().phase, "prepared");
    assert_eq!(w.api.writes.lock().unwrap().len(), 1);
    w.api.offline.store(false, Ordering::SeqCst);
    w.api.code.store(0, Ordering::SeqCst);
    assert_eq!(
        w.resync(&id, &blocked)
            .await
            .unwrap()
            .value
            .requested
            .unwrap()
            .phase,
        "confirmed"
    );
    assert_eq!(w.api.writes.lock().unwrap().len(), 2);
}
#[tokio::test]
async fn recovery_refuses_a_frame_older_than_committed_playback_queue() {
    let w = World::new();
    let blocked = w.blocked().await;
    let store = Arc::new(cumulus_core::storage::StateStore::open(w._root.path()).unwrap());
    let mut queue = cumulus_core::playback::PlaybackQueue::new(Some(store), "9").unwrap();
    queue
        .append(vec![json!({"id":99})], json!({"kind":"songs"}))
        .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    assert!(matches!(
        w.resync(&id, &blocked).await,
        Err(Error::QueueChanged)
    ));
    assert!(w.read(Some(&id)).requested.is_none());
    assert_eq!(w.api.writes.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn local_record_failure_is_visible_through_public_status_without_failing_audio() {
    let mut w = World::new();
    let device = Arc::new(audio::Device::default());
    w.client = w
        .client
        .with_audio_factory(move || Ok(audio::audio_engine(device.clone())));
    let store = Arc::new(cumulus_core::storage::StateStore::open(w._root.path()).unwrap());
    let mut queue = cumulus_core::playback::PlaybackQueue::new(Some(store), "9").unwrap();
    queue
        .replace(
            vec![json!({"id":1,"name":"Synthetic","dt":120000})],
            0,
            json!({"kind":"songs"}),
        )
        .unwrap();
    let db = rusqlite::Connection::open(w._root.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_relay BEFORE INSERT ON state WHEN NEW.kind='relay-publication-v1' BEGIN SELECT RAISE(FAIL,'synthetic recording failure'); END;").unwrap();
    let mut playback = w.client.subscribe_playback(&w.epoch()).await.unwrap();
    w.client
        .player_action(&w.epoch(), PlayerAction::Play)
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if playback.current().unwrap().state
                == cumulus_core::playback::controller::PlaybackStatus::Playing
            {
                break;
            }
            playback.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert!(w.read(None).record_failed);
    assert!(!w.read(None).can_resynchronize);
    assert!(w.api.writes.lock().unwrap().is_empty());
    db.execute_batch("DROP TRIGGER fail_relay").unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            let view = w.read(None);
            if !view.record_failed && view.latest.is_some_and(|row| row.phase == "confirmed") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        playback.current().unwrap().state,
        cumulus_core::playback::controller::PlaybackStatus::Playing
    );
    w.client
        .player_action(&w.epoch(), PlayerAction::Stop)
        .await
        .unwrap();
}
