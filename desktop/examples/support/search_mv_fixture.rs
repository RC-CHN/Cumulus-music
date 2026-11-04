//! Closed MV search playback fixture: the real decoder loads one synthetic file.
use cumulus_core::{Error, Result, playback::engine::*};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const MEDIA: &str = "https://v.test.vod.126.net/search-mv.mp4";
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_SEARCH_MV_PROOF").is_some()
}

pub fn fallback_enabled() -> bool {
    enabled() && std::env::var_os("CUMULUS_SEARCH_MV_FALLBACK_PROOF").is_some()
}

pub fn fallback_config(
    store: Arc<cumulus_core::storage::StateStore>,
) -> Result<Arc<cumulus_core::protocol::mconfig::PcConfig>> {
    use cumulus_core::protocol::{
        api::ApiFuture,
        mconfig::{APPLICATION_KEY, ConfigBackend, ConfigUrl, PcConfig},
    };
    struct Config;
    impl ConfigBackend for Config {
        fn get<'a>(&'a self, url: &'a ConfigUrl) -> ApiFuture<'a, Value> {
            Box::pin(async move {
                assert_eq!(
                    url.as_str(),
                    "https://d5.music.126.net/obj/fixture/mv-fallback.data"
                );
                Ok(json!({"preload#isOpenNoCopyrightMV":true}))
            })
        }
    }
    let config = PcConfig::new(store, Arc::new(Config))?;
    tauri::async_runtime::block_on(config.resolve(&std::collections::BTreeMap::from([(
        "MConfig-Info".into(),
        json!({APPLICATION_KEY:{"version":1,"os":"pc","fullUrl":"https://d5.music.126.net/obj/fixture/mv-fallback.data"}}).to_string(),
    )])))?;
    Ok(config)
}

pub fn fallback_results() -> Value {
    let alt = |id, flag| json!({"type":3,"expInfo":{"mvPlay":flag},"mvInfo":{"id":id,"pic16v9Url":"https://p1.music.126.net/fixture.jpg?authorization=PRIVATE","url":"https://example.invalid/PRIVATE"}});
    let row = |id, name, status, toast, alternative| {
        json!({
            "type":"song","baseInfo":{"simpleSongData":{"id":id,"name":name,"mv":602}},
            "extInfo":{"privilege":{"st":status,"toast":toast},"noCopyrightRcmd":alternative}
        })
    };
    let rows = [
        row(1, "合成替代歌曲", -200, "", alt(json!(601), json!(true))),
        row(2, "合成地区限制", -100, "", alt(json!(601), json!(true))),
        row(3, "合成实验未开", -200, "", alt(json!(601), json!("true"))),
        row(
            4,
            "合成下架说明",
            -100,
            "下架",
            alt(json!("601"), json!(true)),
        ),
        row(5, "合成普通歌曲", 0, "", Value::Null),
        row(
            6,
            "合成无效替代",
            -200,
            "",
            alt(json!("https://example.invalid/PRIVATE"), json!(true)),
        ),
    ];
    json!({"code":200,"data":{"blocks":[{"blockCode":"search_block_song","resources":rows}]}})
}

pub fn calls() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let reply = match path {
        "/api/mlog/detail/v1"
            if *args == json!({"id":"601","type":"2"})
                || (fallback_enabled()
                    && *args == json!({"id":"601","type":"2","videoType":"mp4"})) =>
        {
            json!({"code":200,"data":{"id":"601","resource":{
                "mp":{"fee":0,"payed":0,"pl":480,"dl":0},
                "data":{"id":"601","name":"合成原生 MV","duration":6000,"artists":[],
                        "brs":[{"br":480,"point":0,"size":12207}]}
            }}})
        }
        "/api/song/enhance/play/mv/url" if *args == json!({"id":"601","r":480}) => {
            json!({"code":200,"data":{"id":"601","code":200,"r":480,"url":MEDIA}})
        }
        "/api/mlog/detail/v1" | "/api/song/enhance/play/mv/url" => {
            return Some(Err(Error::InvalidInput("unexpected synthetic MV target")));
        }
        _ => return None,
    };
    CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
    Some(Ok(reply))
}

pub fn create(root: &Path) -> Result<PathBuf> {
    let path = root.join("search-mv-synthetic.mp4");
    std::fs::write(&path, include_bytes!("fixtures/offline-mv.mp4"))?;
    Ok(path)
}

pub struct NativeVideo {
    path: PathBuf,
    engine: MpvVideoEngine,
}

impl NativeVideo {
    pub fn open(path: PathBuf) -> Result<Self> {
        let engine = MpvVideoEngine::open(
            MpvConfig {
                null_output: true,
                ..Default::default()
            },
            480,
            270,
        )?;
        Ok(Self { path, engine })
    }
}

impl AudioEngine for NativeVideo {
    fn load(&mut self, mut request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        if !matches!(&request.source, MediaInput::Url(url) if url.as_str() == MEDIA) {
            return Err(Error::InvalidInput(
                "search MV proof refused non-fixture media",
            ));
        }
        request.source = MediaInput::File(self.path.clone());
        self.engine.load(request, cancel)
    }
    fn pause(&mut self, value: bool) -> Result<()> {
        self.engine.pause(value)
    }
    fn seek(&mut self, value: f64) -> Result<()> {
        self.engine.seek(value)
    }
    fn volume(&mut self, value: f64) -> Result<()> {
        self.engine.volume(value)
    }
    fn speed(&mut self, value: f64) -> Result<()> {
        self.engine.speed(value)
    }
    fn stop(&mut self) -> Result<()> {
        self.engine.stop()
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        self.engine.poll()
    }
}

impl VideoEngine for NativeVideo {
    fn resize(&self, width: u32, height: u32) -> Result<()> {
        self.engine.resize(width, height)
    }
    fn frame_after(&self, sequence: u64) -> Result<Option<Arc<VideoFrame>>> {
        self.engine.frame_after(sequence)
    }
}
