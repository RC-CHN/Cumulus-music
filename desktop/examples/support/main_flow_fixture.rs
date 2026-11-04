//! Closed synthetic fixture: no transport fallback, immutable baseline playlist.
#[path = "main_flow_media.rs"]
pub mod media;
use cumulus_core::{
    Error, Result,
    playback::engine::*,
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

const ORIGINAL: u64 = 41;
const CREATED: u64 = 42;
pub const SOURCE: &str = media::URL;
#[cfg(target_os = "linux")]
const HTTPS_HOST: &str = "cumulus-local-fixture.music.126.net";
const HTTPS_SOURCE: &str = "https://cumulus-local-fixture.music.126.net/synthetic/audio.m4a";
const HTTPS_NEXT_SOURCE: &str = "https://cumulus-local-fixture.music.126.net/synthetic/next.m4a";
const HTTPS_RETRY_SOURCE: &str =
    "https://cumulus-local-fixture.music.126.net/synthetic/audio.m4a?proof-retry=2";

/// The real URL path is test-only and fail-closed outside the loopback fixture.
pub fn ordinary_https() -> bool {
    let enabled = std::env::var("CUMULUS_MAIN_FLOW_HTTPS_PROOF").as_deref() == Ok("1");
    if enabled {
        verify_ordinary_https_isolation();
    }
    enabled
}

#[cfg(not(target_os = "linux"))]
fn verify_ordinary_https_isolation() {
    panic!("ordinary HTTPS proof requires Linux namespaces");
}

#[cfg(target_os = "linux")]
fn verify_ordinary_https_isolation() {
    use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs};
    assert_eq!(
        std::fs::read_to_string("/etc/hosts").unwrap(),
        format!("127.0.0.1 {HTTPS_HOST}\n")
    );
    assert_eq!(
        (HTTPS_HOST, 443)
            .to_socket_addrs()
            .unwrap()
            .collect::<Vec<_>>(),
        [SocketAddr::from((Ipv4Addr::LOCALHOST, 443))]
    );
}

pub struct Service {
    pub media: media::Media,
    calls: Mutex<BTreeMap<String, u64>>,
    playlist: Mutex<Option<Value>>,
    ordinary_https: bool,
    retry_https: bool,
    eof_https: bool,
    loading_step: bool,
    source_authorizations: AtomicUsize,
    source_load_attempts: AtomicUsize,
    loading_seek_released: AtomicBool,
    offline: AtomicBool,
    local_loads: AtomicUsize,
    source_loads: AtomicUsize,
    offline_media_attempts: AtomicUsize,
}

impl Service {
    pub fn new(
        store: Arc<StateStore>,
        ordinary_https: bool,
        retry_https: bool,
        eof_https: bool,
        loading_step: bool,
    ) -> Self {
        Self {
            media: media::Media::new(store),
            ordinary_https,
            retry_https,
            eof_https,
            loading_step,
            source_authorizations: Default::default(),
            source_load_attempts: Default::default(),
            loading_seek_released: Default::default(),
            calls: Default::default(),
            playlist: Default::default(),
            offline: Default::default(),
            local_loads: Default::default(),
            source_loads: Default::default(),
            offline_media_attempts: Default::default(),
        }
    }
    fn first_search_song(&self) -> Value {
        let mut first = song();
        if self.eof_https {
            first["dt"] = json!(12000);
        }
        first
    }
    fn multi_song_queue(&self) -> bool {
        self.eof_https || self.loading_step
    }
    fn playback_url(&self) -> &'static str {
        if self.ordinary_https {
            HTTPS_SOURCE
        } else {
            SOURCE
        }
    }
    fn authorize_playback(&self, id: u64) -> Result<&'static str> {
        let attempt = self.source_authorizations.fetch_add(1, Ordering::AcqRel);
        if self.eof_https {
            return match id {
                1 => Ok(HTTPS_SOURCE),
                2 => Ok(HTTPS_NEXT_SOURCE),
                _ => Err(Error::InvalidInput("unexpected synthetic playback ID")),
            };
        }
        if !self.retry_https || attempt == 0 {
            Ok(self.playback_url())
        } else if attempt == 1 {
            Ok(HTTPS_RETRY_SOURCE)
        } else {
            Err(Error::State(
                "synthetic HTTPS authorization budget exhausted",
            ))
        }
    }
    fn admits_playback(&self, url: &str) -> bool {
        url == self.playback_url()
            || (self.eof_https && url == HTTPS_NEXT_SOURCE)
            || (self.retry_https
                && url == HTTPS_RETRY_SOURCE
                && self.source_authorizations.load(Ordering::Acquire) == 2)
    }
    pub fn second_source_prefetched(&self) -> bool {
        self.eof_https
            && self.source_authorizations.load(Ordering::Acquire) == 2
            && self.source_load_attempts.load(Ordering::Acquire) == 1
    }
    pub fn release_loading_seek(&self) {
        self.loading_seek_released.store(true, Ordering::Release);
    }
    pub fn offline(&self) {
        self.offline.store(true, Ordering::Release);
    }
    pub fn is_offline(&self) -> bool {
        self.offline.load(Ordering::Acquire)
    }
    pub fn facts(&self) -> Value {
        let media = self.media.facts();
        json!({"calls":*self.calls.lock().unwrap(), "disposablePresent":self.playlist.lock().unwrap().is_some(),
            "offline":self.offline.load(Ordering::Acquire), "mediaTransfers":media["count"], "mediaRequests":media["requests"],
            "ordinaryHttps":self.ordinary_https, "retryHttps":self.retry_https, "eofHttps":self.eof_https,
            "sourceAuthorizations":self.source_authorizations.load(Ordering::Acquire),
            "sourceLoadAttempts":self.source_load_attempts.load(Ordering::Acquire), "sourceLoads":self.source_loads.load(Ordering::Acquire), "localLoads":self.local_loads.load(Ordering::Acquire),
            "offlineMediaAttempts":self.offline_media_attempts.load(Ordering::Acquire)})
    }
}
fn song() -> Value {
    json!({"id":1,"name":"合成主流程歌曲","ar":[{"name":"Cumulus"}],"al":{"name":"合成专辑"},"dt":1000,"fee":0})
}
fn eof_song() -> Value {
    json!({"id":2,"name":"合成曲终下一首","ar":[{"name":"Cumulus"}],"al":{"name":"合成专辑"},"dt":12000,"fee":0})
}
fn playlist(id: u64, name: &str) -> Value {
    json!({"id":id,"name":name,"userId":2,"creator":{"userId":2,"nickname":"合成主流程账号"},
        "privacy":10,"specialType":0,"trackCount":0,"trackIds":[],"description":"","tags":[]})
}
fn identifier(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}
impl Api for Service {
    fn request<'a>(&'a self, path: &'a str, p: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            *self.calls.lock().unwrap().entry(path.into()).or_default() += 1;
            if self.offline.load(Ordering::Acquire) {
                return Err(Error::Transport("synthetic API is disconnected".into()));
            }
            Ok(match path {
                "/api/w/nuser/account/get" => {
                    json!({"code":200,"profile":{"userId":2,"nickname":"合成主流程账号"}})
                }
                "/api/rtrs/abt/front/expinfo/list" => json!({"code":200,"data":[]}),
                "/api/relay/setting/show" => json!({"code":200,"data":{"groups":[]}}),
                "/api/song/like/get" => json!({"code":200,"ids":[]}),
                "/api/user/playlist" => {
                    let mut lists = vec![playlist(ORIGINAL, "合成基线歌单")];
                    lists.extend(self.playlist.lock().unwrap().clone());
                    json!({"code":200,"playlist":lists,"more":false})
                }
                "/api/v3/discovery/recommend/songs" => {
                    json!({"code":200,"data":{"dailySongs":[song()]}})
                }
                "/api/search/default/keyword/get" => json!({"code":200,"data":{}}),
                "/api/search/pc/complex/page" => {
                    json!({"code":200,"data":{"blocks":[{"blockCode":"search_block_song","resources":[{"type":"song","baseInfo":song()}]}],"hasMore":false}})
                }
                "/api/search/song/list/page" => {
                    let mut rows = vec![
                        json!({"type":"song","baseInfo":{"simpleSongData":self.first_search_song()}}),
                    ];
                    if self.multi_song_queue() {
                        rows.push(json!({"type":"song","baseInfo":{"simpleSongData":eof_song()}}));
                    }
                    json!({"code":200,"data":{"totalCount":rows.len(),"resources":rows,"hasMore":false}})
                }
                "/api/search/resource/lyric" => {
                    json!({"code":200,"data":{"totalCount":1,"resources":[{"song":song(),"lyrics":"合成主流程歌词"}],"hasMore":false}})
                }
                "/api/v3/song/detail" => {
                    if self.multi_song_queue() {
                        let packed = p["c"]
                            .as_str()
                            .ok_or(Error::InvalidInput("missing synthetic song IDs"))?;
                        let ids: Vec<Value> = serde_json::from_str(packed)?;
                        let songs = ids
                            .iter()
                            .map(|row| match identifier(&row["id"]) {
                                Some(1) => Ok(self.first_search_song()),
                                Some(2) => Ok(eof_song()),
                                _ => Err(Error::InvalidInput("unexpected synthetic song ID")),
                            })
                            .collect::<Result<Vec<_>>>()?;
                        json!({"code":200,"songs":songs})
                    } else {
                        json!({"code":200,"songs":[song()]})
                    }
                }
                "/api/song/lyric/v1" => json!({"code":200,"lrc":{"lyric":"[00:00.00]合成测试"}}),
                "/api/song/enhance/player/url/v1" => {
                    let id = if self.multi_song_queue() {
                        let packed = p["ids"]
                            .as_str()
                            .ok_or(Error::InvalidInput("missing synthetic authorization ID"))?;
                        let ids: Vec<Value> = serde_json::from_str(packed)?;
                        match ids.as_slice() {
                            [only] => identifier(only)
                                .ok_or(Error::InvalidInput("invalid synthetic authorization ID"))?,
                            _ => {
                                return Err(Error::InvalidInput(
                                    "unexpected synthetic authorization batch",
                                ));
                            }
                        }
                    } else {
                        1
                    };
                    json!({"code":200,"data":[{"id":id,"url":self.authorize_playback(id)?,"code":200,"md5":"synthetic","level":"standard","expi":120}]})
                }
                "/api/song/enhance/download/url/v1" => {
                    json!({"code":200,"data":{"id":1,"url":SOURCE,"code":200,"type":"flac","size":media::DATA.len(),"md5":media::MD5,"level":"lossless"}})
                }
                "/api/v6/playlist/detail" => {
                    let list = match identifier(&p["id"]) {
                        Some(ORIGINAL) => Some(playlist(ORIGINAL, "合成基线歌单")),
                        Some(CREATED) => self.playlist.lock().unwrap().clone(),
                        _ => None,
                    };
                    if let Some(list) = list {
                        json!({"code":200,"playlist":list})
                    } else {
                        json!({"code":404})
                    }
                }
                "/api/playlist/catalogue" => {
                    json!({"code":200,"categories":{"0":"风格"},"sub":[{"name":"电子","category":0}]})
                }
                "/api/playlist/create" => {
                    let mut state = self.playlist.lock().unwrap();
                    if state.is_some()
                        || p["name"] != "Cumulus Synthetic Flow"
                        || p["privacy"] != 10
                        || identifier(&p["uid"]) != Some(2)
                    {
                        return Err(Error::InvalidInput(
                            "only one named private synthetic playlist may be created",
                        ));
                    }
                    let list = playlist(CREATED, "Cumulus Synthetic Flow");
                    *state = Some(list.clone());
                    json!({"code":200,"id":CREATED,"playlist":list})
                }
                "/api/batch" => {
                    let mut state = self.playlist.lock().unwrap();
                    let list = state
                        .as_mut()
                        .ok_or(Error::State("missing disposable playlist"))?;
                    let mut result = json!({"code":200});
                    for (path, field, key) in [
                        ("/api/playlist/update/name", "name", "name"),
                        ("/api/playlist/desc/update", "desc", "description"),
                        ("/api/playlist/tags/update", "tags", "tags"),
                    ] {
                        let value: Value = serde_json::from_str(
                            p[path]
                                .as_str()
                                .ok_or(Error::Response("missing synthetic batch field"))?,
                        )?;
                        if identifier(&value["id"]) != Some(CREATED) {
                            return Err(Error::InvalidInput("cannot edit baseline playlist"));
                        }
                        list[key] = if key == "tags" {
                            json!(
                                value[field]
                                    .as_str()
                                    .unwrap_or("")
                                    .split(';')
                                    .filter(|v| !v.is_empty())
                                    .collect::<Vec<_>>()
                            )
                        } else {
                            value[field].clone()
                        };
                        result[path] = json!({"code":200});
                    }
                    result
                }
                "/api/v1/playlist/manipulate/tracks" => {
                    if identifier(&p["pid"]) != Some(CREATED)
                        || p["trackIds"] != r#"["1"]"#
                        || p["imme"] != true
                    {
                        return Err(Error::InvalidInput(
                            "only the synthetic song and disposable playlist may be changed",
                        ));
                    }
                    let mut state = self.playlist.lock().unwrap();
                    let list = state
                        .as_mut()
                        .ok_or(Error::State("missing disposable playlist"))?;
                    match p["op"].as_str() {
                        Some("add") if list["trackIds"] == json!([]) => {
                            list["trackIds"] = json!([{"id":1}]);
                            list["trackCount"] = json!(1);
                        }
                        Some("del") if list["trackIds"] == json!([{"id":1}]) => {
                            list["trackIds"] = json!([]);
                            list["trackCount"] = json!(0);
                        }
                        _ => {
                            return Err(Error::State(
                                "unexpected synthetic playlist membership transition",
                            ));
                        }
                    }
                    json!({"code":200})
                }
                "/api/playlist/delete" => {
                    let mut state = self.playlist.lock().unwrap();
                    if identifier(&p["pid"]) != Some(CREATED)
                        || !state
                            .as_ref()
                            .is_some_and(|list| list["trackIds"] == json!([]))
                    {
                        return Err(Error::InvalidInput(
                            "only the empty disposable playlist may be deleted",
                        ));
                    }
                    *state = None;
                    json!({"code":200})
                }
                _ => return Err(Error::Response("endpoint not in closed main-flow fixture")),
            })
        })
    }
}

pub struct Audio {
    inner: MpvEngine,
    seed: PathBuf,
    downloads: PathBuf,
    fixture: Arc<Service>,
}
impl Audio {
    pub fn open(seed: PathBuf, downloads: PathBuf, fixture: Arc<Service>) -> Result<Self> {
        Ok(Self {
            inner: MpvEngine::open(MpvConfig {
                null_output: true,
                ..Default::default()
            })?,
            seed,
            downloads,
            fixture,
        })
    }
}
impl AudioEngine for Audio {
    fn load(&mut self, mut request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        let offline = self.fixture.offline.load(Ordering::Acquire);
        let local = match &request.source {
            MediaInput::Url(url) if !offline && self.fixture.admits_playback(url.as_str()) => {
                if !self.fixture.ordinary_https {
                    request.source = MediaInput::File(self.seed.clone());
                }
                false
            }
            MediaInput::File(path)
                if offline
                    && path
                        .canonicalize()?
                        .starts_with(self.downloads.canonicalize()?) =>
            {
                true
            }
            _ => {
                return Err(Error::InvalidInput(
                    "main flow proof refused non-fixture audio",
                ));
            }
        };
        if !local {
            let attempt = self
                .fixture
                .source_load_attempts
                .fetch_add(1, Ordering::AcqRel);
            if attempt == 0 && !self.fixture.ordinary_https {
                while !self.fixture.loading_seek_released.load(Ordering::Acquire) {
                    cancel.check()?;
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
        let loaded = self.inner.load(request, cancel)?;
        if self.fixture.ordinary_https && !local && loaded.codec.as_deref() != Some("aac") {
            let _ = self.inner.stop();
            return Err(Error::State("ordinary HTTPS fixture must decode AAC"));
        }
        let counter = if local {
            &self.fixture.local_loads
        } else {
            &self.fixture.source_loads
        };
        counter.fetch_add(1, Ordering::Release);
        Ok(loaded)
    }
    fn pause(&mut self, v: bool) -> Result<()> {
        self.inner.pause(v)
    }
    fn seek(&mut self, v: f64) -> Result<()> {
        self.inner.seek(v)
    }
    fn volume(&mut self, v: f64) -> Result<()> {
        self.inner.volume(v)
    }
    fn speed(&mut self, v: f64) -> Result<()> {
        self.inner.speed(v)
    }
    fn stop(&mut self) -> Result<()> {
        self.inner.stop()
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        self.inner.poll()
    }
}

impl cumulus_core::download::transfer::MediaBackend for Service {
    fn get(
        &self,
        request: cumulus_core::download::transfer::MediaRequest,
    ) -> ApiFuture<'_, cumulus_core::download::transfer::MediaReply> {
        Box::pin(async move {
            if self.offline.load(Ordering::Acquire) {
                self.offline_media_attempts.fetch_add(1, Ordering::AcqRel);
                return Err(Error::Transport("synthetic media is disconnected".into()));
            }
            self.media.get(request)
        })
    }
}
