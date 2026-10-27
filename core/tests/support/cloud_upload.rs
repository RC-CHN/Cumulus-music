#![allow(dead_code)]
#[path = "nos.rs"]
pub mod nos;
#[path = "oss.rs"]
pub mod oss;
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    cloud::{nos::*, upload::*},
    library::{AudioMetadata, probe::AudioProbe},
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};
pub const CHECK: &str = "/api/cloud/upload/check";
pub const ALLOC: &str = "/api/whale/token/alloc";
pub const ATTACH: &str = "/api/nos/token/alloc";
pub const META: &str = "/api/upload/cloud/info/v2";
pub const STATUS: &str = "/api/v1/cloud/music/status";
pub const PUBLISH: &str = "/api/cloud/pub/v2";
pub const LIST: &str = "/api/v1/cloud/get";
pub const DATA: &[u8] = b"abcdefghi";
#[derive(Default)]
pub struct Model {
    pub calls: Mutex<Vec<(String, Value)>>,
    pub replies: Mutex<VecDeque<(&'static str, Result<Value>)>>,
    pub gate: Mutex<Option<(&'static str, Arc<nos::Gate>)>>,
}
impl Model {
    pub fn push(&self, path: &'static str, response: Value) {
        self.replies.lock().unwrap().push_back((path, Ok(response)));
    }
    pub fn lost(&self, path: &'static str) {
        self.replies
            .lock()
            .unwrap()
            .push_back((path, Err(Error::Transport("fixture lost reply".into()))));
    }
    pub fn calls(&self) -> Vec<(String, Value)> {
        self.calls.lock().unwrap().clone()
    }
    pub fn check(&self, need: bool) {
        self.push(
            CHECK,
            json!({"code":200,"needUpload":need,"songId":"opaque-upload-ticket-not-a-catalog-id"}),
        );
    }
    pub fn allocate(&self) {
        self.push(ALLOC, json!({"code":200,"data":allocation("audio",1)}));
    }
    pub fn metadata(&self, ready: bool) {
        self.push(META,json!({"code":200,"songId":700,"exists":ready,"waitTime":0,"privateCloud":{"songId":700,"simpleSong":{"mst":if ready{9}else{0}}}}));
    }
    pub fn published(&self) {
        self.push(PUBLISH, json!({"code":201,"privateCloud":{"songId":700}}));
    }
    pub fn listed(&self, found: bool) {
        self.push(LIST,json!({"code":200,"more":false,"data":if found{json!([{"songId":700,"fileName":"input.flac"}])}else{json!([])}}));
    }
}
impl Api for Model {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((path.into(), payload));
            let (expected, result) = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| panic!("unexpected fixture request {path}"));
            assert_eq!(path, expected);
            let gate = {
                let mut g = self.gate.lock().unwrap();
                if g.as_ref().is_some_and(|(p, _)| *p == path) {
                    g.take().map(|(_, g)| g)
                } else {
                    None
                }
            };
            if let Some(gate) = gate {
                gate.entered.notify_one();
                gate.release.acquire().await.unwrap().forget();
            }
            result
        })
    }
}
pub fn allocation(key: &str, resource: u64) -> Value {
    json!({"channel":1,"bucket":"bucket-test","objectKey":key,"token":"scoped-test-token","resourceId":resource,"docId":resource})
}
#[derive(Default)]
pub struct Objects {
    pub objects: Mutex<BTreeMap<String, Arc<nos::Backend>>>,
    pub discoveries: AtomicUsize,
}
impl Objects {
    pub fn object(&self, key: &str) -> Arc<nos::Backend> {
        self.objects
            .lock()
            .unwrap()
            .entry(format!("/bucket-test/{key}"))
            .or_default()
            .clone()
    }
    pub fn calls(&self) -> usize {
        self.objects
            .lock()
            .unwrap()
            .values()
            .map(|b| b.calls().len())
            .sum()
    }
}
impl NosBackend for Objects {
    fn discover<'a>(&'a self, bucket: &'a str) -> ApiFuture<'a, NosNode> {
        Box::pin(async move {
            assert_eq!(bucket, "bucket-test");
            self.discoveries.fetch_add(1, Ordering::SeqCst);
            NosNode::parse("https://nosup-test.127.net")
        })
    }
    fn request(&self, request: NosRequest) -> ApiFuture<'_, Value> {
        Box::pin(async move {
            let path = url::Url::parse(request.url()).unwrap().path().to_owned();
            let backend = self
                .objects
                .lock()
                .unwrap()
                .entry(path)
                .or_default()
                .clone();
            backend.request(request).await
        })
    }
}
pub struct Probe;
impl AudioProbe for Probe {
    fn inspect(&self, _: &Path) -> Result<AudioMetadata> {
        Ok(AudioMetadata {
            format: Some("flac".into()),
            duration: 2.0,
            title: "Fixture cloud audio".into(),
            album: "Fixture album".into(),
            artists: vec!["First artist".into(), "Second artist".into()],
            bitrate: Some(320000),
            sample_rate: Some(44100),
            channels: Some(2),
            container: "flac".into(),
            cover_bytes: None,
        })
    }
}
pub struct Clock(pub AtomicU64);
impl UploadClock for Clock {
    fn now_ms(&self) -> Result<u64> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}
pub struct Fixture {
    pub root: tempfile::TempDir,
    pub source: PathBuf,
    pub store: Arc<StateStore>,
    pub session: Arc<AccountSession>,
    pub api: Arc<Model>,
    pub objects: Arc<Objects>,
    pub oss: Arc<oss::Backend>,
    pub clock: Arc<Clock>,
    pub manager: CloudUploads,
}
impl Fixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("input.flac");
        std::fs::write(&source, DATA).unwrap();
        let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
        let api = Arc::new(Model::default());
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        let objects = Arc::new(Objects::default());
        let oss = Arc::new(oss::Backend::default());
        let clock = Arc::new(Clock(AtomicU64::new(1000)));
        let manager = CloudUploads::with_adapters(
            session.clone(),
            UploadAdapters {
                nos: objects.clone(),
                oss: oss.clone(),
                probe: Arc::new(Probe),
                clock: clock.clone(),
                chunk_size: 3,
                oss_part_size: oss::PART,
                oss_concurrency: 3,
            },
        )
        .unwrap();
        Self {
            root,
            source,
            store,
            session,
            api,
            objects,
            oss,
            clock,
            manager,
        }
    }
    pub fn reopened(&self) -> CloudUploads {
        let store = Arc::new(StateStore::open(self.store.directory()).unwrap());
        let session = Arc::new(AccountSession::new(self.api.clone(), Some(store)));
        session
            .adopt(
                self.api.clone(),
                json!({"userId":1}),
                &session.token().unwrap(),
            )
            .unwrap();
        CloudUploads::with_adapters(
            session,
            UploadAdapters {
                nos: self.objects.clone(),
                oss: self.oss.clone(),
                probe: Arc::new(Probe),
                clock: self.clock.clone(),
                chunk_size: 7,
                oss_part_size: 2 * oss::PART,
                oss_concurrency: 1,
            },
        )
        .unwrap()
    }
    pub async fn create(&self) -> String {
        self.manager
            .create(&self.source, Default::default())
            .await
            .unwrap()
            .id
    }
    pub fn tick(&self, ms: u64) {
        self.clock.0.fetch_add(ms, Ordering::SeqCst);
    }
}
