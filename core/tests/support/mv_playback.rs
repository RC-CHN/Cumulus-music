#![allow(dead_code)]
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    mv::playback::{VideoController, VideoEngineFactory, VideoSnapshot, VideoStatus},
    playback::engine::*,
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
pub struct Server {
    pub calls: Mutex<Vec<String>>,
    pub fail_detail: AtomicBool,
    pub hold: AtomicBool,
    pub started: tokio::sync::Notify,
    pub release: tokio::sync::Notify,
}
impl Api for Server {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            self.calls.lock().unwrap().push(path.into());
            let id = &payload["id"];
            match path {
                "/api/mlog/detail/v1" => {
                    if self.fail_detail.load(Ordering::Acquire) {
                        return Err(Error::Transport("fixture offline".into()));
                    }
                    Ok(
                        json!({"code":200,"data":{"id":id,"resource":{"mp":{"fee":0,"payed":0,"pl":1080,"dl":1080},"data":{"id":id,"name":"Fixture MV","duration":120000,"artists":[],"brs":[{"br":240,"point":0,"size":12207}]}}}}),
                    )
                }
                "/api/song/enhance/play/mv/url" => {
                    if self.hold.swap(false, Ordering::AcqRel) {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    Ok(
                        json!({"code":200,"data":{"id":id,"code":200,"r":240,"url":"https://v.test.vod.126.net/video?secret=fixture","size":12207}}),
                    )
                }
                "/api/song/enhance/download/mv/url" => Ok(
                    json!({"code":200,"data":{"id":id,"code":200,"r":240,"url":"https://v.test.vod.126.net/video?secret=fixture","size":12207,"md5":""}}),
                ),
                _ => panic!("unexpected MV endpoint {path}"),
            }
        })
    }
}
pub struct Device {
    pub position: f64,
    pub seek_landing: Option<f64>,
    pub paused: bool,
    pub volume: f64,
    pub reject_volume: Option<f64>,
    pub speed: f64,
    pub source_file: bool,
    pub source_path: Option<std::path::PathBuf>,
    pub hold_load: Arc<AtomicBool>,
    pub load_started: Arc<tokio::sync::Notify>,
    pub stopped: bool,
    pub hold_stop: Arc<AtomicBool>,
    pub stop_started: Arc<tokio::sync::Notify>,
    pub events: Vec<EngineEvent>,
    width: u32,
    height: u32,
}
struct Fake(Arc<Mutex<Device>>);
impl Drop for Fake {
    fn drop(&mut self) {
        self.0.lock().unwrap().stopped = true;
    }
}
impl AudioEngine for Fake {
    fn load(&mut self, r: LoadRequest, c: &Cancellation) -> Result<LoadedMedia> {
        c.check()?;
        let mut d = self.0.lock().unwrap();
        d.position = r.start;
        d.source_file = matches!(r.source, MediaInput::File(_));
        d.source_path = match r.source {
            MediaInput::File(path) => Some(path),
            _ => None,
        };
        let (hold, started) = (d.hold_load.clone(), d.load_started.clone());
        drop(d);
        started.notify_one();
        while hold.load(Ordering::Acquire) {
            c.check()?;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        Ok(LoadedMedia {
            entry_id: 1,
            duration: Some(120.0),
            codec: Some("fixture".into()),
        })
    }
    fn pause(&mut self, p: bool) -> Result<()> {
        self.0.lock().unwrap().paused = p;
        Ok(())
    }
    fn seek(&mut self, s: f64) -> Result<()> {
        let mut device = self.0.lock().unwrap();
        device.position = device.seek_landing.unwrap_or(s);
        Ok(())
    }
    fn volume(&mut self, v: f64) -> Result<()> {
        let mut device = self.0.lock().unwrap();
        if device.reject_volume == Some(v) {
            return Err(Error::State("synthetic video volume failure"));
        }
        device.volume = v;
        Ok(())
    }
    fn speed(&mut self, v: f64) -> Result<()> {
        self.0.lock().unwrap().speed = v;
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        let (hold, started) = {
            let d = self.0.lock().unwrap();
            (d.hold_stop.clone(), d.stop_started.clone())
        };
        started.notify_one();
        while hold.load(Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        self.0.lock().unwrap().stopped = true;
        Ok(())
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        let mut d = self.0.lock().unwrap();
        Ok(EngineSnapshot {
            entry_id: Some(1),
            position: Some(d.position),
            duration: Some(120.0),
            paused: d.paused,
            buffering: false,
            volume: Some(d.volume),
            speed: Some(d.speed),
            audio_output: Some("fixture".into()),
            events: std::mem::take(&mut d.events),
        })
    }
}
impl VideoEngine for Fake {
    fn resize(&self, w: u32, h: u32) -> Result<()> {
        let mut d = self.0.lock().unwrap();
        d.width = w;
        d.height = h;
        Ok(())
    }
    fn frame_after(&self, s: u64) -> Result<Option<Arc<VideoFrame>>> {
        let d = self.0.lock().unwrap();
        Ok((s < 1).then(|| {
            Arc::new(VideoFrame {
                sequence: 1,
                width: d.width,
                height: d.height,
                rgba: vec![255; d.width as usize * d.height as usize * 4].into(),
            })
        }))
    }
}
pub struct Fixture {
    pub root: tempfile::TempDir,
    pub store: Arc<StateStore>,
    pub session: Arc<AccountSession>,
    pub api: Arc<Server>,
    pub devices: Arc<Mutex<Vec<Arc<Mutex<Device>>>>>,
    pub factory: VideoEngineFactory,
    pub player: Arc<VideoController>,
    pub hold_load: Arc<AtomicBool>,
    pub load_started: Arc<tokio::sync::Notify>,
}
impl Fixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
        let api = Arc::new(Server {
            calls: Mutex::new(vec![]),
            fail_detail: AtomicBool::new(false),
            hold: AtomicBool::new(false),
            started: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        });
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        let devices = Arc::new(Mutex::new(vec![]));
        let list = devices.clone();
        let hold_load = Arc::new(AtomicBool::new(false));
        let load_started = Arc::new(tokio::sync::Notify::new());
        let held = hold_load.clone();
        let started = load_started.clone();
        let factory: VideoEngineFactory = Arc::new(move || {
            let device = Arc::new(Mutex::new(Device {
                position: 0.0,
                seek_landing: None,
                paused: true,
                volume: 60.0,
                reject_volume: None,
                speed: 1.0,
                source_file: false,
                source_path: None,
                hold_load: held.clone(),
                load_started: started.clone(),
                stopped: false,
                hold_stop: Arc::new(AtomicBool::new(false)),
                stop_started: Arc::new(tokio::sync::Notify::new()),
                events: vec![],
                width: 128,
                height: 72,
            }));
            list.lock().unwrap().push(device.clone());
            Ok(Box::new(Fake(device)))
        });
        let player = Arc::new(
            VideoController::start(session.clone(), session.token().unwrap(), factory.clone())
                .unwrap(),
        );
        Self {
            root,
            store,
            session,
            api,
            devices,
            factory,
            player,
            hold_load,
            load_started,
        }
    }
    pub fn another(&self) -> VideoController {
        VideoController::start(
            self.session.clone(),
            self.session.token().unwrap(),
            self.factory.clone(),
        )
        .unwrap()
    }
    pub fn last(&self) -> Arc<Mutex<Device>> {
        self.devices.lock().unwrap().last().unwrap().clone()
    }
}
pub async fn until(player: &VideoController, status: VideoStatus) -> VideoSnapshot {
    tokio::time::timeout(std::time::Duration::from_secs(4), async {
        loop {
            let view = player.snapshot().unwrap();
            if view.status == status {
                return view;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("video state timeout")
}
