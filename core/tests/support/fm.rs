use cumulus_core::{
    Error, Result,
    account::AccountSession,
    fm::*,
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
};
use tokio::sync::Notify;

#[derive(Clone, Copy)]
pub enum Fault {
    Lost,
    Before,
    Reject(i64),
}
#[derive(Default)]
pub struct Backend {
    pub calls: Mutex<Vec<(String, Value)>>,
    pub batches: Mutex<VecDeque<Vec<Value>>>,
    pub responses: Mutex<HashMap<String, Value>>,
    pub trash: Mutex<BTreeSet<String>>,
    pub blacklist: Mutex<FmBlacklist>,
    pub faults: Mutex<HashMap<String, Fault>>,
    pub hold: Mutex<Option<String>>,
    pub started: Notify,
    pub release: Notify,
    pub bad_trash: AtomicU8,
}
pub fn song(id: u64) -> Value {
    json!({"id":id,"name":format!("song-{id}"),"alg":format!("alg-{id}"),"artists":[{"id":id%3}]})
}
impl Backend {
    pub fn batch(&self, ids: &[u64]) {
        self.batches
            .lock()
            .unwrap()
            .push_back(ids.iter().map(|&id| song(id)).collect());
    }
    pub fn count(&self, path: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == path)
            .count()
    }
    pub fn fault(&self, path: &str, fault: Fault) {
        self.faults.lock().unwrap().insert(path.into(), fault);
    }
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), payload.clone()));
            let fault = self.faults.lock().unwrap().remove(path);
            match fault {
                Some(Fault::Before) => return Err(Error::Transport("fixture before send".into())),
                Some(Fault::Reject(code)) => return Ok(json!({"code":code})),
                _ => {}
            }
            let value = match path {
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/fixture.mp3?secret=PRIVATE","md5":"abc","expi":120,"level":payload["level"]}]})
                }
                "/api/music-vip-membership/client/vip/info" => {
                    json!({"code":200,"data":{"now":1793030000000u64,"userId":9}})
                }
                "/api/v1/radio/get" | "/api/tag/tab/fm/recommends" => {
                    json!({"code":200,"data":self.batches.lock().unwrap().pop_front().unwrap_or_default()})
                }
                "/api/v1/radio/skip" => json!({"code":200}),
                "/api/music/dislike/get" => {
                    let b = self.blacklist.lock().unwrap();
                    json!({"code":200,"data":{"songIdList":b.songs,"artistIdList":b.artists}})
                }
                "/api/music-blacklist/add" | "/api/music-blacklist/delete" => {
                    assert_eq!(payload["contentType"], "artist");
                    let mut b = self.blacklist.lock().unwrap();
                    for id in payload["contentIdList"].as_array().unwrap() {
                        let id = id.as_str().unwrap();
                        if path.ends_with("/add") {
                            b.artists.insert(id.into());
                        } else {
                            b.artists.remove(id);
                        }
                    }
                    json!({"code":200})
                }
                "/api/v3/radio/trash/get" => {
                    let rows: Vec<_> = self.trash.lock().unwrap().iter().cloned().collect();
                    let size = payload["pagesize"].as_u64().unwrap() as usize;
                    let from = payload["from"].as_u64().unwrap() as usize;
                    let mut data: Vec<Value> = rows
                        .iter()
                        .skip(from * size)
                        .take(size)
                        .map(|id| json!({"id":id}))
                        .collect();
                    let bad = self.bad_trash.load(Ordering::SeqCst);
                    if bad == 1 && from > 0 && !data.is_empty() {
                        data[0] = json!({"id":rows[0]});
                    }
                    if bad == 3 && from == 0 {
                        data.clear();
                    }
                    let count = if bad == 4 {
                        0
                    } else {
                        rows.len() + usize::from(bad == 2 && from > 0)
                    };
                    json!({"code":200,"count":count,"addTime":1000+from,"data":data})
                }
                "/api/radio/trash/add" => {
                    self.trash
                        .lock()
                        .unwrap()
                        .insert(payload["songId"].as_str().unwrap().into());
                    json!({"code":200})
                }
                "/api/radio/trash/del/batch" => {
                    let mut trash = self.trash.lock().unwrap();
                    for id in payload["songIds"].as_array().unwrap() {
                        trash.remove(id.as_str().unwrap());
                    }
                    json!({"code":200})
                }
                "/api/personalized/dislike/reason" => {
                    json!({"code":200,"data":[{"type":"fixed","reason":"推荐重复"},{"type":"fixed","reason":"推荐不感兴趣"},{"type":"artist","id":"1","reason":"屏蔽歌手：fixture artist"}]})
                }
                _ => self
                    .responses
                    .lock()
                    .unwrap()
                    .get(path)
                    .cloned()
                    .unwrap_or_else(|| panic!("unexpected FM endpoint {path}")),
            };
            let hold = {
                let mut h = self.hold.lock().unwrap();
                if h.as_deref() == Some(path) {
                    h.take();
                    true
                } else {
                    false
                }
            };
            if hold {
                self.started.notify_one();
                self.release.notified().await;
            }
            if matches!(fault, Some(Fault::Lost)) {
                return Err(Error::Transport("fixture lost successful reply".into()));
            }
            Ok(value)
        })
    }
}
pub struct Clock(pub AtomicU64);
impl FmClock for Clock {
    fn now_ms(&self) -> Result<u64> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}
pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub api: Arc<Backend>,
    pub store: Arc<StateStore>,
    pub session: Arc<AccountSession>,
    pub clock: Arc<Clock>,
    pub fm: Arc<FmService>,
}
impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let api = Arc::new(Backend::default());
        let store = Arc::new(StateStore::open(dir.path()).unwrap());
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
        let clock = Arc::new(Clock(AtomicU64::new(10000)));
        let fm = Arc::new(FmService::with_clock(session.clone(), clock.clone()).unwrap());
        Self {
            dir,
            api,
            store,
            session,
            clock,
            fm,
        }
    }
    pub fn tick(&self) {
        self.clock.0.fetch_add(251, Ordering::SeqCst);
    }
    pub async fn preload(&self, ids: &[u64]) -> FmQueue {
        self.api.batch(ids);
        self.fm
            .preload_for(
                &self.session.token().unwrap(),
                FmSource::Personal,
                FmRights::default(),
            )
            .await
            .unwrap()
    }
    pub fn adopt(&self, id: u64) {
        self.session
            .adopt(
                self.api.clone(),
                json!({"userId":id}),
                &self.session.token().unwrap(),
            )
            .unwrap();
    }
}
