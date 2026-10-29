use cumulus_core::{
    Error, Result,
    account::{AccountSession, SessionToken},
    playback::{
        controller::{PlaybackCommand, PlaybackController, PlaybackSnapshot},
        engine::{
            AudioEngine, Cancellation, EngineEvent, EngineSnapshot, LoadRequest, LoadedMedia,
        },
        source::{ResolverClock, SourceResolver},
    },
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

#[derive(Default)]
pub struct Clock(pub AtomicU64);
impl ResolverClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}
#[derive(Default)]
pub struct Backend {
    pub gains: Mutex<std::collections::BTreeMap<u64, Value>>,
    pub calls: Mutex<Vec<(u64, String)>>,
    pub hold: AtomicU64,
    pub deny_lossless: AtomicBool,
    pub trial: AtomicBool,
    pub started: Notify,
    pub release: Notify,
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(path, "/api/song/enhance/player/url/v1");
            let ids: Vec<Value> = serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap();
            let id = ids[0]
                .as_u64()
                .or_else(|| ids[0].as_str().and_then(|s| s.parse().ok()))
                .unwrap();
            let level = payload["level"].as_str().unwrap().to_owned();
            self.calls.lock().unwrap().push((id, level.clone()));
            if self.hold.load(Ordering::SeqCst) == id {
                self.started.notify_one();
                self.release.notified().await;
            }
            let code = if level == "lossless" && self.deny_lossless.load(Ordering::SeqCst) {
                403
            } else {
                200
            };
            Ok(json!({"code":200,"data":[{
                "id":id,"code":code,"url":"https://m1.music.126.net/fixture.mp3?secret=private",
                "md5":"abcd","expi":10,"level":"standard",
                "gain":self.gains.lock().unwrap().get(&id).cloned().unwrap_or(Value::Null),
                "freeTrialInfo":if self.trial.load(Ordering::SeqCst) {json!({"start":30,"end":60})} else {Value::Null}
            }]}))
        })
    }
}
impl Backend {
    pub fn count(&self, id: u64) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(n, _)| *n == id)
            .count()
    }
}
pub struct DeviceState {
    pub view: EngineSnapshot,
    pub loads: usize,
    pub stops: usize,
    pub duration: f64,
    pub bounds: Vec<(f64, Option<f64>)>,
    pub pauses: Vec<bool>,
}
pub struct Device {
    pub loudness_supported: AtomicBool,
    pub loudness_fail: AtomicBool,
    pub loudness_clear_fail: AtomicBool,
    pub loudness: Mutex<Option<cumulus_core::effects::loudness::LoudnessSettings>>,
    pub loudness_at_unpause: Mutex<Vec<Option<cumulus_core::effects::loudness::LoudnessSettings>>>,
    pub equalizer_supported: AtomicBool,
    pub equalizer_fail: AtomicBool,
    pub equalizer_hold: AtomicBool,
    pub equalizer_started: Notify,
    pub equalizer: Mutex<Option<cumulus_core::effects::equalizer::EqualizerSettings>>,
    pub equalizer_at_unpause:
        Mutex<Vec<Option<cumulus_core::effects::equalizer::EqualizerSettings>>>,
    pub equalizer_clears: AtomicU64,
    pub state: Mutex<DeviceState>,
    pub hold_load: AtomicBool,
    pub fail_seek: AtomicBool,
    pub fail_unpause: AtomicBool,
    pub started: Notify,
}
impl Default for Device {
    fn default() -> Self {
        Self {
            loudness_supported: AtomicBool::new(false),
            loudness_fail: AtomicBool::new(false),
            loudness_clear_fail: AtomicBool::new(false),
            loudness: Mutex::new(None),
            loudness_at_unpause: Mutex::new(Vec::new()),
            equalizer_supported: AtomicBool::new(false),
            equalizer_fail: AtomicBool::new(false),
            equalizer_hold: AtomicBool::new(false),
            equalizer_started: Notify::new(),
            equalizer: Mutex::new(None),
            equalizer_at_unpause: Mutex::new(Vec::new()),
            equalizer_clears: AtomicU64::new(0),
            state: Mutex::new(DeviceState {
                view: EngineSnapshot {
                    entry_id: None,
                    position: None,
                    duration: None,
                    paused: true,
                    buffering: false,
                    volume: Some(100.0),
                    speed: Some(1.0),
                    audio_output: Some("fixture".into()),
                    events: vec![],
                },
                loads: 0,
                stops: 0,
                duration: 120.0,
                bounds: vec![],
                pauses: vec![],
            }),
            hold_load: AtomicBool::new(false),
            fail_seek: AtomicBool::new(false),
            fail_unpause: AtomicBool::new(false),
            started: Notify::new(),
        }
    }
}
struct Engine(Arc<Device>);
#[allow(dead_code)]
pub fn audio_engine(device: Arc<Device>) -> Box<dyn AudioEngine> {
    Box::new(Engine(device))
}
impl AudioEngine for Engine {
    fn apply_loudness(
        &mut self,
        settings: cumulus_core::effects::loudness::LoudnessSettings,
    ) -> Result<bool> {
        if !self.0.loudness_supported.load(Ordering::SeqCst) {
            return Ok(false);
        }
        if self.0.loudness_fail.load(Ordering::SeqCst) {
            return Err(Error::State("injected loudness failure"));
        }
        *self.0.loudness.lock().unwrap() = Some(settings);
        Ok(true)
    }
    fn clear_loudness(&mut self) -> Result<()> {
        if self.0.loudness_clear_fail.load(Ordering::SeqCst) {
            return Err(Error::State("injected loudness cleanup failure"));
        }
        *self.0.loudness.lock().unwrap() = None;
        Ok(())
    }
    fn apply_equalizer(
        &mut self,
        settings: cumulus_core::effects::equalizer::EqualizerSettings,
    ) -> Result<bool> {
        if !self.0.equalizer_supported.load(Ordering::SeqCst) {
            return Ok(false);
        }
        self.0.equalizer_started.notify_one();
        while self.0.equalizer_hold.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }
        if self.0.equalizer_fail.load(Ordering::SeqCst) {
            return Err(Error::State("injected equalizer failure"));
        }
        *self.0.equalizer.lock().unwrap() = Some(settings);
        Ok(true)
    }
    fn clear_equalizer(&mut self) -> Result<()> {
        *self.0.equalizer.lock().unwrap() = None;
        self.0.equalizer_clears.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn load(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        self.0.started.notify_one();
        while self.0.hold_load.load(Ordering::SeqCst) {
            cancel.check()?;
            std::thread::sleep(Duration::from_millis(2));
        }
        cancel.check()?;
        let mut state = self.0.state.lock().unwrap();
        state.loads += 1;
        let entry_id = state.loads as i64;
        state.bounds.push((request.start, request.end));
        state.view.entry_id = Some(entry_id);
        state.view.position = Some(request.start);
        state.view.duration = Some(state.duration);
        state.view.paused = true;
        Ok(LoadedMedia {
            entry_id,
            duration: Some(state.duration),
            codec: Some("fixture".into()),
        })
    }
    fn pause(&mut self, paused: bool) -> Result<()> {
        if !paused && self.0.fail_unpause.load(Ordering::SeqCst) {
            return Err(Error::State("injected native unpause failure"));
        }
        if !paused {
            self.0
                .loudness_at_unpause
                .lock()
                .unwrap()
                .push(*self.0.loudness.lock().unwrap());
            self.0
                .equalizer_at_unpause
                .lock()
                .unwrap()
                .push(*self.0.equalizer.lock().unwrap());
        }
        let mut state = self.0.state.lock().unwrap();
        state.pauses.push(paused);
        state.view.paused = paused;
        Ok(())
    }
    fn seek(&mut self, seconds: f64) -> Result<()> {
        if self.0.fail_seek.load(Ordering::SeqCst) {
            return Err(Error::State("injected native seek failure"));
        }
        self.0.state.lock().unwrap().view.position = Some(seconds);
        Ok(())
    }
    fn volume(&mut self, value: f64) -> Result<()> {
        self.0.state.lock().unwrap().view.volume = Some(value);
        Ok(())
    }
    fn speed(&mut self, value: f64) -> Result<()> {
        self.0.state.lock().unwrap().view.speed = Some(value);
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        let mut state = self.0.state.lock().unwrap();
        state.stops += 1;
        state.view.entry_id = None;
        state.view.position = None;
        state.view.paused = true;
        Ok(())
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        let mut state = self.0.state.lock().unwrap();
        let view = state.view.clone();
        state.view.events.clear();
        Ok(view)
    }
}
pub struct Fixture {
    pub temp: tempfile::TempDir,
    pub store: Arc<StateStore>,
    pub api: Arc<Backend>,
    pub clock: Arc<Clock>,
    pub session: Arc<AccountSession>,
    pub resolver: Arc<SourceResolver>,
    pub device: Arc<Device>,
    pub player: Arc<PlaybackController>,
}
impl Fixture {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(temp.path()).unwrap());
        let api = Arc::new(Backend::default());
        let session = Arc::new(AccountSession::new(api.clone(), None));
        session
            .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
        let clock = Arc::new(Clock::default());
        let resolver = Arc::new(SourceResolver::with_clock(
            session.clone(),
            None,
            clock.clone(),
        ));
        let device = Arc::new(Device::default());
        let player = Arc::new(
            PlaybackController::start(
                session.clone(),
                store.clone(),
                resolver.clone(),
                Box::new(Engine(device.clone())),
            )
            .unwrap(),
        );
        Self {
            temp,
            store,
            api,
            clock,
            session,
            resolver,
            device,
            player,
        }
    }
    pub fn token(&self) -> SessionToken {
        self.session.token().unwrap()
    }
    pub async fn command(&self, command: PlaybackCommand) -> Result<PlaybackSnapshot> {
        tokio::time::timeout(
            Duration::from_secs(4),
            self.player.command(&self.token(), command),
        )
        .await
        .expect("controller response timed out")
    }
    pub fn start(
        &self,
        command: PlaybackCommand,
    ) -> tokio::task::JoinHandle<Result<PlaybackSnapshot>> {
        let player = self.player.clone();
        let token = self.token();
        tokio::spawn(async move { player.command(&token, command).await })
    }
    pub fn another_player(&self) -> Arc<PlaybackController> {
        Arc::new(
            PlaybackController::start(
                self.session.clone(),
                self.store.clone(),
                self.resolver.clone(),
                Box::new(Engine(Arc::new(Device::default()))),
            )
            .unwrap(),
        )
    }
    pub fn sql(&self, sql: &str) {
        rusqlite::Connection::open(self.temp.path().join("core.sqlite3"))
            .unwrap()
            .execute_batch(sql)
            .unwrap();
    }
}
pub fn replace(ids: &[u64]) -> PlaybackCommand {
    PlaybackCommand::Replace {
        resources: ids.iter().map(|id| json!({"id":id,"dt":120000})).collect(),
        index: 0,
        source: json!({"kind":"test"}),
    }
}
pub async fn until(
    player: &PlaybackController,
    condition: impl Fn(&PlaybackSnapshot) -> bool,
) -> PlaybackSnapshot {
    let mut view = player.subscribe();
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let state = view.borrow().clone();
            if condition(&state) {
                return state;
            }
            view.changed().await.unwrap();
        }
    })
    .await
    .expect("player state did not converge")
}
pub fn song(view: &PlaybackSnapshot) -> Option<u64> {
    view.selected
        .as_ref()
        .and_then(|s| s.resource["id"].as_u64())
}
pub fn event(device: &Device, entry_id: i64, reason: cumulus_core::playback::engine::EndReason) {
    device
        .state
        .lock()
        .unwrap()
        .view
        .events
        .push(EngineEvent::Ended {
            entry_id,
            reason,
            error: 0,
        });
}
pub fn root(error: &Error) -> &Error {
    error.root()
}
