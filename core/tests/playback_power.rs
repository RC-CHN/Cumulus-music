#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod fixture_playback;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{PlaySelection, PlayerAction, PlayerView},
    },
    membership::Quality,
    playback::{
        controller::PlaybackStatus,
        engine::{EndReason, EngineEvent},
        keep_awake::{PlaybackPowerRequest, PlaybackPowerSink},
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
        pc_experiments::PcExperimentGroups,
    },
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

const WAIT: Duration = Duration::from_secs(5);
const DETAIL: &str = "/api/v3/song/detail";
const MEDIA: &str = "/api/song/enhance/player/url/v1";
use PlaybackPowerRequest::{Clear, KeepSystemAndDisplayAwake as Keep};

#[derive(Default)]
struct FakePower(Mutex<Vec<PlaybackPowerRequest>>);

impl PlaybackPowerSink for FakePower {
    fn submit(&self, request: PlaybackPowerRequest) {
        self.0.lock().unwrap().push(request);
    }
}

impl FakePower {
    fn transitions(&self) -> Vec<PlaybackPowerRequest> {
        let requests = self.0.lock().unwrap();
        let mut transitions: Vec<_> = requests.iter().copied().collect();
        transitions.dedup();
        if transitions.first() == Some(&Clear) {
            transitions.remove(0);
        }
        transitions
    }

    async fn expect(&self, expected: &[PlaybackPowerRequest]) {
        tokio::time::timeout(WAIT, async {
            loop {
                let actual = self.transitions();
                if actual == expected {
                    return;
                }
                assert!(
                    expected.starts_with(&actual),
                    "unexpected power transitions {actual:?}"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "expected power transitions {expected:?}, observed {:?}",
                self.transitions()
            )
        });
    }
}

struct Fixture {
    root: tempfile::TempDir,
    registry: Arc<ProfileRegistry>,
    session: Arc<AccountSession>,
    device: Arc<fixture_playback::Device>,
    sink: Arc<FakePower>,
    audio_initializations: Arc<AtomicUsize>,
}

impl Fixture {
    fn new(api: Arc<Backend>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
        session
            .adopt(api, json!({"userId":7}), &session.token().unwrap())
            .unwrap();
        Self {
            root,
            registry,
            session,
            device: Arc::default(),
            sink: Arc::default(),
            audio_initializations: Arc::default(),
        }
    }

    fn client(&self) -> MusicClient {
        self.client_with_registry(self.registry.clone())
    }

    fn client_with_registry(&self, registry: Arc<ProfileRegistry>) -> MusicClient {
        let device = self.device.clone();
        let audio = self.audio_initializations.clone();
        MusicClient::with_session(self.session.clone(), registry)
            .with_audio_factory(move || {
                audio.fetch_add(1, Ordering::SeqCst);
                Ok(fixture_playback::audio_engine(device.clone()))
            })
            .with_playback_power_sink(self.sink.clone())
    }
}

#[derive(Default)]
struct Backend {
    group: Option<&'static str>,
    assignment_reads: AtomicUsize,
    requests: Mutex<Vec<String>>,
    hold_url: AtomicBool,
    url_started: Notify,
    url_release: Notify,
}

impl Backend {
    fn new(group: Option<&'static str>) -> Arc<Self> {
        Arc::new(Self {
            group,
            ..Self::default()
        })
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Api for Backend {
    fn playback_power_experiment(&self) -> ApiFuture<'_, Option<PcExperimentGroups>> {
        Box::pin(async {
            self.assignment_reads.fetch_add(1, Ordering::SeqCst);
            self.group
                .map(|group| {
                    PcExperimentGroups::refreshed(&json!({
                        "data":[{"expName":"PH-PC-keepAwake","expGroupName":group}]
                    }))
                })
                .transpose()
        })
    }

    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(path.to_owned());
            match path {
                DETAIL => {
                    let songs: Vec<Value> =
                        serde_json::from_str(args["c"].as_str().unwrap()).unwrap();
                    assert_eq!(songs.len(), 1);
                    assert_eq!(songs[0]["id"], "101");
                    Ok(json!({"code":200,"songs":[{
                        "id":"101","name":"Fixture audio","dt":120000
                    }]}))
                }
                MEDIA => {
                    let ids: Vec<Value> =
                        serde_json::from_str(args["ids"].as_str().unwrap()).unwrap();
                    assert_eq!(ids.len(), 1);
                    assert!(ids[0] == "101" || ids[0] == 101);
                    self.url_started.notify_one();
                    if self.hold_url.load(Ordering::SeqCst) {
                        self.url_release.notified().await;
                    }
                    Ok(json!({"code":200,"data":[{
                        "id":"101","code":200,"url":"https://m1.music.126.net/fixture.mp3",
                        "md5":"fixture","expi":120,"level":"standard"
                    }]}))
                }
                _ => Err(Error::State("unexpected non-fixture request")),
            }
        })
    }
}

fn selection() -> PlaySelection {
    PlaySelection::Songs {
        ids: vec!["101".into()],
        index: 0,
        quality: Quality::Standard,
    }
}

async fn expect_playback(
    client: &MusicClient,
    epoch: &str,
    expected: PlaybackStatus,
) -> PlayerView {
    tokio::time::timeout(WAIT, async {
        loop {
            let player = client.player_snapshot(epoch).await.unwrap();
            if player.state == expected {
                return player;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("player did not reach {expected:?}"))
}

fn end_decoder(device: &fixture_playback::Device, reason: EndReason, error: i32) {
    let mut state = device.state.lock().unwrap();
    let entry_id = state.view.entry_id.unwrap();
    state.view.events.push(EngineEvent::Ended {
        entry_id,
        reason,
        error,
    });
}

#[tokio::test]
async fn device_power_preference_is_local_and_remains_effective_after_assignment_changes() {
    let assigned = Backend::new(Some("t1"));
    let f = Fixture::new(assigned.clone());
    let client = f.client();
    let unconfirmed = client.set_playback_power("1", true).await.unwrap();
    assert!(!unconfirmed.available && !unconfirmed.enabled);
    assert_eq!(assigned.assignment_reads.load(Ordering::SeqCst), 0);
    let initial = client.playback_power("1").await.unwrap();
    assert!(initial.available);
    assert!(!initial.enabled);
    let changed = client.set_playback_power("1", true).await.unwrap();
    assert!(changed.available && changed.enabled);
    assert_eq!(f.audio_initializations.load(Ordering::SeqCst), 0);
    assert!(assigned.requests().is_empty());
    assert!(assigned.assignment_reads.load(Ordering::SeqCst) > 0);
    assert!(f.sink.transitions().is_empty());
    let state = f.registry.store();
    assert_eq!(
        state
            .get("local-device", "playback-power-v1", "enabled")
            .unwrap(),
        Some(json!(true))
    );
    assert_eq!(
        state.get("7", "playback-power-v1", "enabled").unwrap(),
        None
    );

    let control = Backend::new(Some("c"));
    f.session
        .adopt(
            control.clone(),
            json!({"userId":8}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        client.playback_power("1").await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.set_playback_power("1", false).await,
        Err(Error::StaleResponse)
    ));
    drop(client);

    let reopened =
        f.client_with_registry(ProfileRegistry::open(f.root.path(), ApiOrigin::Pc).unwrap());
    let current = reopened.playback_power("2").await.unwrap();
    assert!(!current.available && current.enabled);
    let hidden_write = reopened.set_playback_power("2", false).await.unwrap();
    assert!(!hidden_write.available && hidden_write.enabled);
    assert_eq!(f.audio_initializations.load(Ordering::SeqCst), 0);
    assert!(control.requests().is_empty());
    reopened.play_selection("2", selection()).await.unwrap();
    expect_playback(&reopened, "2", PlaybackStatus::Playing).await;
    f.sink.expect(&[Keep]).await;
    reopened
        .player_action("2", PlayerAction::Stop)
        .await
        .unwrap();
    f.sink.expect(&[Keep, Clear]).await;
    assert_eq!(control.requests(), [DETAIL, MEDIA]);
    assert!(assigned.requests().is_empty());
    assert_eq!(f.audio_initializations.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn power_follows_authorized_unpaused_audio_and_releases_on_pause_stop_and_native_error() {
    let api = Backend::new(Some("t1"));
    let f = Fixture::new(api.clone());
    let client = Arc::new(f.client());
    assert!(client.playback_power("1").await.unwrap().available);
    assert!(client.set_playback_power("1", true).await.unwrap().enabled);
    assert_eq!(f.audio_initializations.load(Ordering::SeqCst), 0);
    api.hold_url.store(true, Ordering::SeqCst);
    f.device.hold_load.store(true, Ordering::SeqCst);
    let start = client.clone();
    let pending = tokio::spawn(async move { start.play_selection("1", selection()).await });
    tokio::time::timeout(WAIT, api.url_started.notified())
        .await
        .unwrap();
    expect_playback(&client, "1", PlaybackStatus::Preparing).await;
    assert!(f.sink.transitions().is_empty());
    assert_eq!(f.device.state.lock().unwrap().loads, 0);
    api.hold_url.store(false, Ordering::SeqCst);
    api.url_release.notify_one();
    tokio::time::timeout(WAIT, f.device.started.notified())
        .await
        .unwrap();
    expect_playback(&client, "1", PlaybackStatus::Loading).await;
    // The official model publishes Playing before entering nativePlay.
    // Rust waits for source authorization and current intent, not decoder return.
    assert_eq!(f.device.state.lock().unwrap().loads, 0);
    f.sink.expect(&[Keep]).await;
    f.device.hold_load.store(false, Ordering::SeqCst);
    tokio::time::timeout(WAIT, pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let playing = expect_playback(&client, "1", PlaybackStatus::Playing).await;
    f.sink.expect(&[Keep]).await;

    f.device.state.lock().unwrap().view.buffering = true;
    let buffering = expect_playback(&client, "1", PlaybackStatus::Buffering).await;
    assert_eq!(buffering.play_id, playing.play_id);
    assert_eq!(f.sink.transitions(), [Keep]);
    f.device.state.lock().unwrap().view.buffering = false;
    expect_playback(&client, "1", PlaybackStatus::Playing).await;
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    expect_playback(&client, "1", PlaybackStatus::Paused).await;
    f.sink.expect(&[Keep, Clear]).await;
    client
        .player_action("1", PlayerAction::Pause { paused: false })
        .await
        .unwrap();
    expect_playback(&client, "1", PlaybackStatus::Playing).await;
    f.sink.expect(&[Keep, Clear, Keep]).await;
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    f.sink.expect(&[Keep, Clear, Keep, Clear]).await;

    client.play_selection("1", selection()).await.unwrap();
    expect_playback(&client, "1", PlaybackStatus::Playing).await;
    f.sink.expect(&[Keep, Clear, Keep, Clear, Keep]).await;
    end_decoder(&f.device, EndReason::Error, -15);
    expect_playback(&client, "1", PlaybackStatus::Error).await;
    f.sink
        .expect(&[Keep, Clear, Keep, Clear, Keep, Clear])
        .await;
    assert!(client.playback_power("1").await.unwrap().enabled);
    assert!(
        api.requests()
            .iter()
            .all(|path| [DETAIL, MEDIA].contains(&path.as_str()))
    );
}
