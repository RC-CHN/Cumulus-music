//! Synthetic relay responses and an explicit fixture-only media substitution.
//! Production source resolution still authorizes the synthetic CDN URL; only
//! this test engine swaps that exact URL for generated PCM before libmpv load.
use cumulus_core::{
    Error, Result,
    playback::engine::*,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
pub struct Library {
    base: super::settings_fixture::Library,
    mode: Mutex<String>,
    pub pulls: AtomicUsize,
    pub setting_writes: AtomicUsize,
    pub publications: Mutex<Vec<Value>>,
    publication_mode: Mutex<String>,
    publication_configs: AtomicUsize,
    pub network_calls: AtomicUsize,
    enabled: Mutex<bool>,
    fail_setting_read: std::sync::atomic::AtomicBool,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            base: Default::default(),
            mode: Mutex::new("song".into()),
            pulls: AtomicUsize::new(0),
            setting_writes: AtomicUsize::new(0),
            publications: Mutex::new(vec![]),
            publication_mode: Mutex::new(String::new()),
            publication_configs: AtomicUsize::new(0),
            network_calls: AtomicUsize::new(0),
            enabled: Mutex::new(true),
            fail_setting_read: std::sync::atomic::AtomicBool::new(false),
        }
    }
}
impl Library {
    pub fn mode(&self, mode: &str) {
        if mode.starts_with("publication-") {
            *self.publication_mode.lock().unwrap() = mode.into();
            self.publication_configs.store(0, Ordering::SeqCst);
            return;
        }
        if mode == "setting-off" {
            *self.enabled.lock().unwrap() = false;
        }
        if mode == "setting-normal" {
            self.fail_setting_read.store(false, Ordering::SeqCst);
        }
        *self.mode.lock().unwrap() = mode.into();
    }
}
const MEDIA: &str = "https://m1.music.126.net/cumulus-relay-fixture.wav";
fn voice() -> Value {
    json!({"id":"5","name":"合成声音","mainSong":{"id":"1005","duration":30000},"duration":30000,"radio":{"id":8,"name":"合成播客"}})
}
impl Api for Library {
    fn local_device_id(&self) -> Result<Option<String>> {
        Ok(Some("SYNTHETIC_PRIVATE-local".into()))
    }
    fn request<'a>(&'a self, path: &'a str, p: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.network_calls.fetch_add(1, Ordering::SeqCst);
            let publication = self.publication_mode.lock().unwrap().clone();
            let mode = self.mode.lock().unwrap().clone();
            if mode == "offline" && (path.starts_with("/api/relay/") || path.contains("expinfo")) {
                return Err(Error::Transport("synthetic offline".into()));
            }
            Ok(match path {
                "/api/relay/songlist/submit" | "/api/relay/play/state/submit" => {
                    let field = if path.ends_with("songlist/submit") {
                        "songListSubmitReq"
                    } else {
                        "playStateSubmitReq"
                    };
                    self.publications.lock().unwrap().push(serde_json::from_str(
                        p[field]
                            .as_str()
                            .ok_or(Error::InvalidInput("publication JSON string missing"))?,
                    )?);
                    if publication == "publication-rejected" {
                        return Ok(json!({"code":403}));
                    }
                    if publication == "publication-lost" {
                        return Err(Error::Transport(
                            "synthetic publication response lost".into(),
                        ));
                    }
                    json!({"code":200})
                }
                "/api/rtrs/abt/front/expinfo/list" => {
                    json!({"code":200,"data":[{"expName":"device_connect","expGroupName":if mode=="excluded"{"control"}else{"t2"}}]})
                }
                "/api/relay/config/get" => {
                    if publication == "publication-prepared-offline"
                        && self.publication_configs.fetch_add(1, Ordering::SeqCst) > 0
                    {
                        return Err(Error::Transport(
                            "synthetic publication preflight unavailable".into(),
                        ));
                    }
                    json!({"code":200,"data":{"enable":mode!="disabled" && *self.enabled.lock().unwrap(),"radioProgressInterval":30,"snapshotSize":1000}})
                }
                "/api/relay/setting/show" => {
                    if self.fail_setting_read.load(Ordering::SeqCst) {
                        return Err(Error::Transport(
                            "synthetic read-after-write failure".into(),
                        ));
                    }
                    let enabled = if mode == "setting-missing" {
                        Value::Null
                    } else {
                        json!(mode != "disabled" && *self.enabled.lock().unwrap())
                    };
                    json!({"code":200,"data":{"groups":[{"deviceSettingList":[{"enable":enabled}]}]}})
                }
                "/api/relay/setting/update" => {
                    if p["targetDeviceId"] != "SYNTHETIC_PRIVATE-local"
                        || p["settingKey"] != "RELAY_SWITCH"
                    {
                        return Err(Error::InvalidInput(
                            "synthetic relay setting target mismatch",
                        ));
                    }
                    self.setting_writes.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(900)).await;
                    if mode == "setting-rejected" {
                        return Ok(json!({"code":403}));
                    }
                    if mode != "setting-lost-unapplied" {
                        *self.enabled.lock().unwrap() = p["enable"]
                            .as_bool()
                            .ok_or(Error::InvalidInput("synthetic setting bool missing"))?;
                    }
                    if mode == "setting-lost-applied" || mode == "setting-lost-unapplied" {
                        return Err(Error::Transport("synthetic lost switch response".into()));
                    }
                    if mode == "setting-read-failed" {
                        self.fail_setting_read.store(true, Ordering::SeqCst);
                    }
                    json!({"code":200})
                }
                "/api/link/position/show/resource" => {
                    json!({"code":200,"data":{"commonResourceList":if mode=="empty"{vec![]}else{vec![json!({"generalizedObject":{"deviceId":"SYNTHETIC_PRIVATE","deviceName":"另一台测试设备","resourceName":if mode=="voice"{"合成播客 · 第五期"}else{"合成歌曲与播客混合队列"},"reconnectTitle":"接着上次的音乐","btnDesc":"继续播放","opaque":"SYNTHETIC_PRIVATE"}})]}}})
                }
                "/api/relay/play/pull" => {
                    self.pulls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
                    if mode == "rejected" {
                        return Ok(json!({"code":403}));
                    }
                    if mode == "lost" {
                        return Err(Error::Transport("synthetic lost pull response".into()));
                    }
                    json!({"code":200,"data":{"relayMode":"snapshot","resources":[{"id":"2","type":"song"},{"id":"5","type":"voice"}],"currentPlayState":{"resourceId":if mode=="voice"{"5"}else{"2"},"resourceType":if mode=="voice"{"voice"}else{"song"},"progress":12500},"playMode":"list_loop"}})
                }
                "/api/dj/program/batch/detail" => json!({"code":200,"data":[voice()]}),
                "/api/dj/program/detail" => json!({"code":200,"program":voice()}),
                "/api/dj/play/url/check" => {
                    json!({"code":200,"data":{"voiceId":p["voiceId"],"songId":"1005","canPlay":true}})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> = serde_json::from_str(p["ids"].as_str().unwrap())?;
                    json!({"code":200,"data":[{"id":ids[0],"url":MEDIA,"code":200,"md5":"fixture","level":"standard","expi":120}]})
                }
                "/api/song/enhance/privilege" if mode == "permission" => {
                    return Err(Error::Transport(
                        "synthetic metadata permission failure".into(),
                    ));
                }
                _ => return self.base.request(path, p, cache).await,
            })
        })
    }
}
pub fn wave(root: &Path) -> Result<PathBuf> {
    let path = root.join("relay-synthetic.wav");
    let samples = 48000_u32 * 30;
    let bytes = samples * 2;
    let mut out = Vec::with_capacity(bytes as usize + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(bytes + 36).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&48000_u32.to_le_bytes());
    out.extend_from_slice(&96000_u32.to_le_bytes());
    out.extend_from_slice(&2_u16.to_le_bytes());
    out.extend_from_slice(&16_u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&bytes.to_le_bytes());
    for n in 0..samples {
        let value = (f64::sin(n as f64 * 440.0 * std::f64::consts::TAU / 48000.0) * 2000.0) as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    std::fs::write(&path, out)?;
    Ok(path)
}
pub struct NativeAudio {
    inner: MpvEngine,
    wave: PathBuf,
}
impl NativeAudio {
    pub fn open(wave: PathBuf) -> Result<Self> {
        Ok(Self {
            inner: MpvEngine::open(MpvConfig {
                null_output: true,
                ..Default::default()
            })?,
            wave,
        })
    }
}
impl AudioEngine for NativeAudio {
    fn load(&mut self, mut request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        if !matches!(&request.source,MediaInput::Url(url)if url.as_str()==MEDIA) {
            return Err(Error::InvalidInput("relay proof refused non-fixture media"));
        }
        request.source = MediaInput::File(self.wave.clone());
        self.inner.load(request, cancel)
    }
    fn pause(&mut self, p: bool) -> Result<()> {
        self.inner.pause(p)
    }
    fn seek(&mut self, p: f64) -> Result<()> {
        self.inner.seek(p)
    }
    fn volume(&mut self, p: f64) -> Result<()> {
        self.inner.volume(p)
    }
    fn speed(&mut self, p: f64) -> Result<()> {
        self.inner.speed(p)
    }
    fn stop(&mut self) -> Result<()> {
        self.inner.stop()
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        self.inner.poll()
    }
}
