//! Closed synthetic PEQ wire and material fixture, real libmpv PCM path.
use cumulus_core::{
    Error, Result,
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    effects::{equalizer::abi::EqualizerStats, parametric::ParametricPlan},
    playback::engine::{
        AudioEngine, Cancellation, EngineSnapshot, LoadRequest, LoadedMedia, MediaInput, MpvConfig,
        MpvEngine,
    },
    protocol::api::ApiFuture,
};
use serde_json::{Value, json};
use std::{
    io::{Cursor, Read},
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

const ID: u64 = 300;
pub(super) const URL: &str = "https://m.test.music.126.net/synthetic-bound-peq.ncae";
pub(super) const MD5: &str = "3d7172c914c75c5b32df037f967142a9";
pub(super) const AUDIO_URL: &str = "https://m1.music.126.net/cumulus-relay-fixture.wav";
// Same frozen, synthetic NCAE as the independent bound PEQ probe.
pub(super) const NCAE: &str = concat!(
    "4e4341454f000000000000000100010009020100076352515057924ea58f1ff3ec",
    "1c3887235c6c92bd5a53ca960d2d51ed640714dece76e313f9e42a644e1e1ed9",
    "4e0ec25f14c3085cab80dc7d6188e5d87472637298cd3ae778afb67376e9a909",
    "ac32171644744961",
);

#[derive(Default)]
struct Trace {
    calls: Vec<Value>,
    material_gets: u32,
    starts: u32,
    stops: u32,
    attaches: u32,
    detaches: u32,
    bypasses: u32,
    bypass_revision: Option<u64>,
    stats: Option<EqualizerStats>,
    native_attached: bool,
    injected_faults: u32,
    loads: u32,
}

static TRACE: std::sync::LazyLock<Mutex<Trace>> =
    std::sync::LazyLock::new(|| Mutex::new(Trace::default()));
static FAIL_NEXT_POLL: AtomicBool = AtomicBool::new(false);

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_EFFECT_PEQ_NATIVE_PROOF").is_some()
}

pub fn inject_runtime_fault() -> bool {
    if (!enabled() && std::env::var_os("CUMULUS_EFFECT_DEVICE_NATIVE_PROOF").is_none())
        || std::env::var_os("CUMULUS_EFFECT_PEQ_RUNTIME_FAULT").is_none()
    {
        return false;
    }
    let trace = TRACE.lock().unwrap();
    if !trace.native_attached
        || trace.stats.is_none_or(|stats| stats.processed_samples == 0)
        || trace.injected_faults != 0
    {
        return false;
    }
    !FAIL_NEXT_POLL.swap(true, Ordering::AcqRel)
}

fn sound() -> Value {
    json!({"id":ID,"name":"合成参数均衡音效","subTitle":"封闭夹具","soundType":3,
        "soundMaterialList":[{"type":1,"size":NCAE.len()/2,"md5":MD5,"link":URL}]})
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let mut trace = TRACE.lock().unwrap();
    // The reopened process has no simulated active lease in memory. Allow
    // only the ordinary empty catalogue and one explicit compensating stop.
    if std::env::var_os("CUMULUS_PROOF_REOPEN").is_some() {
        let data = match path {
            "/api/usertool/sound/queryAllSound" if args == &json!({"exposeType":5}) => json!([]),
            "/api/vipauth/app/interests/auth/stop"
                if args == &json!({"interestsType":127,"interestsId":"300","type":1})
                    && trace.stops == 0
                    && trace.starts == 0
                    && !trace.native_attached =>
            {
                trace.stops += 1;
                json!(true)
            }
            _ if path.starts_with("/api/vipauth/app/interests/")
                || path.starts_with("/api/usertool/home/sound")
                || path.starts_with("/api/usertool/sound/") =>
            {
                trace.calls.push(json!({"path":path,"result":"unexpected"}));
                return Some(Err(Error::Response(
                    "unapproved reopened synthetic PEQ route",
                )));
            }
            _ => return None,
        };
        trace.calls.push(json!({"path":path,"result":"ack"}));
        return Some(Ok(json!({"code":200,"data":data})));
    }
    let data = match path {
        "/api/usertool/sound/queryAllSound" if args == &json!({"exposeType":5}) => {
            json!([sound()])
        }
        "/api/usertool/home/sound/info"
            if args == &json!({"reqListStr":r#"[{"soundId":"300","soundType":10}]"#}) =>
        {
            json!({"soundList":[{"soundId":"300","soundType":10,"pcSoundDetail":sound()}]})
        }
        "/api/vipauth/app/interests/auth/query"
            if args == &json!({"queryInfo":{"interestsMap":{"127":[ID]}}}) =>
        {
            json!([{"interestsId":ID,"interestsType":127,
                "authResp":{"authResult":{"canUse":true,"canUseType":11}}}])
        }
        "/api/vipauth/app/interests/recyclequery"
            if args == &json!({"queryInfo":{"127":["300"]}}) =>
        {
            json!({"127":[{"interestsId":ID,"canRecycle":false}]})
        }
        "/api/vipauth/app/interests/auth/set"
            if args == &json!({"interestsType":127,"interestsId":"300","type":0})
                && trace.starts == 0
                && !trace.native_attached =>
        {
            trace.starts += 1;
            json!(true)
        }
        "/api/vipauth/app/interests/auth/stop"
            if args == &json!({"interestsType":127,"interestsId":"300","type":1})
                && trace.starts == 1
                && trace.stops < 2
                && !trace.native_attached
                && trace.detaches == 1 =>
        {
            trace.stops += 1;
            if trace.stops == 1 {
                trace.calls.push(json!({"path":path,"result":"lost"}));
                return Some(Err(Error::Transport(
                    "synthetic stop acknowledgement lost".into(),
                )));
            }
            json!(true)
        }
        _ if path.starts_with("/api/vipauth/app/interests/")
            || path.starts_with("/api/usertool/home/sound")
            || path.starts_with("/api/usertool/sound/") =>
        {
            trace.calls.push(json!({"path":path,"result":"unexpected"}));
            return Some(Err(Error::Response("unapproved synthetic PEQ route")));
        }
        _ => return None,
    };
    trace.calls.push(json!({"path":path,"result":"ack"}));
    Some(Ok(json!({"code":200,"data":data})))
}

pub struct Material;
struct Body(Cursor<Vec<u8>>);
impl MediaBody for Body {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move { Ok(self.0.read(buffer)?) })
    }
}
impl MediaBackend for Material {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            if std::env::var_os("CUMULUS_PROOF_REOPEN").is_some()
                || request.url.as_str() != URL
                || request.offset != 0
                || request.if_range.is_some()
            {
                return Err(Error::Response("unapproved synthetic PEQ material"));
            }
            let mut trace = TRACE.lock().unwrap();
            trace.material_gets += 1;
            if trace.material_gets != 1 {
                return Err(Error::Response("duplicate synthetic PEQ material"));
            }
            let bytes = hex::decode(NCAE).expect("frozen synthetic NCAE");
            Ok(MediaReply {
                status: 200,
                headers: [("content-length".into(), bytes.len().to_string())].into(),
                body: Box::new(Body(Cursor::new(bytes))),
            })
        })
    }
}

pub struct Audio {
    engine: MpvEngine,
    wave: PathBuf,
}
impl Audio {
    pub fn open(wave: PathBuf, library: PathBuf) -> Result<Self> {
        Ok(Self {
            engine: MpvEngine::open(MpvConfig {
                null_output: true,
                ..Default::default()
            })?
            .with_effects_library(library),
            wave,
        })
    }
}
impl AudioEngine for Audio {
    fn load(&mut self, mut request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        if !matches!(&request.source, MediaInput::Url(url) if url.as_str() == AUDIO_URL) {
            return Err(Error::InvalidInput(
                "native PEQ proof refused non-fixture audio",
            ));
        }
        request.source = MediaInput::File(self.wave.clone());
        let loaded = self.engine.load(request, cancel)?;
        TRACE.lock().unwrap().loads += 1;
        Ok(loaded)
    }
    fn pause(&mut self, paused: bool) -> Result<()> {
        self.engine.pause(paused)
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
        if FAIL_NEXT_POLL.swap(false, Ordering::AcqRel) {
            TRACE.lock().unwrap().injected_faults += 1;
            return Err(Error::ParametricProcessorFailed);
        }
        let view = self.engine.poll()?;
        if let Ok(stats) = self.engine.parametric_equalizer_stats() {
            TRACE.lock().unwrap().stats = Some(stats);
        }
        Ok(view)
    }
    fn apply_parametric_component(&mut self, plan: ParametricPlan) -> Result<bool> {
        let attached = self.engine.apply_parametric_component(plan)?;
        if attached {
            let mut trace = TRACE.lock().unwrap();
            trace.native_attached = true;
            trace.stats = None;
            trace.bypass_revision = None;
            trace.attaches += 1;
        }
        Ok(attached)
    }
    fn bypass_parametric_component(&mut self) -> Result<()> {
        let before = self.engine.parametric_equalizer_stats()?;
        self.engine.bypass_parametric_component()?;
        let after = self.engine.parametric_equalizer_stats()?;
        if after.revision <= before.revision {
            return Err(Error::State(
                "native PEQ bypass did not publish a new revision",
            ));
        }
        let mut trace = TRACE.lock().unwrap();
        trace.bypasses += 1;
        trace.bypass_revision = Some(after.revision);
        trace.stats = Some(after);
        Ok(())
    }
    fn clear_parametric_component(&mut self) -> Result<()> {
        self.engine.clear_parametric_component()?;
        let mut trace = TRACE.lock().unwrap();
        if trace.native_attached {
            if self.engine.parametric_equalizer_stats().is_ok() {
                return Err(Error::State("native PEQ still attached after clear"));
            }
            trace.native_attached = false;
            trace.bypass_revision = None;
            trace.detaches += 1;
        }
        Ok(())
    }
}

pub fn report() -> Value {
    let trace = TRACE.lock().unwrap();
    json!({
        "processId":std::process::id(),
        "calls":trace.calls,
        "materialGets":trace.material_gets,
        "decoderLoads":trace.loads,
        "starts":trace.starts,
        "stops":trace.stops,
        "attaches":trace.attaches,
        "detaches":trace.detaches,
        "bypasses":trace.bypasses,
        "bypassRevision":trace.bypass_revision,
        "observedRevision":trace.stats.map(|stats|stats.observed_revision),
        "revision":trace.stats.map(|stats|stats.revision),
        "nativeAttached":trace.native_attached,
        "processedSamples":trace.stats.map(|stats| stats.processed_samples).unwrap_or(0),
        "failedBlocks":trace.stats.map(|stats| stats.failed_blocks).unwrap_or(0),
        "injectedFaults":trace.injected_faults,
    })
}
