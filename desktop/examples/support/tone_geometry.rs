//! Sealed measurement of the stock mono LADSPA adapter. No new renderer API.
use cumulus_core::{
    Error, Result,
    effects::equalizer::{EqualizerSettings, abi::EqualizerStats},
    playback::engine::{
        AudioEngine, Cancellation, EndReason, EngineEvent, EngineSnapshot, LoadRequest,
        LoadedMedia, MediaInput, MpvConfig, MpvEngine,
    },
};
use libloading::Library;
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex, OnceLock},
};

const CAP: usize = 2048;
const FIELDS: usize = 6;
const SOURCES: [(u64, &str, u32, u16, u32); 2] = [
    (
        1,
        "https://m1.music.126.net/cumulus-tone-mono.wav",
        44100,
        1,
        264737,
    ),
    (
        2,
        "https://m1.music.126.net/cumulus-tone-stereo.wav",
        48000,
        2,
        144137,
    ),
];

type Cursor = unsafe extern "C" fn() -> u64;
type Snapshot = unsafe extern "C" fn(*mut u64, usize) -> isize;
struct Witness {
    _library: Library,
    cursor: Cursor,
    snapshot: Snapshot,
}
static WITNESS: OnceLock<Witness> = OnceLock::new();
#[derive(Default)]
struct Trace {
    opened: bool,
    stats: Option<EqualizerStats>,
    source_calls: Vec<u64>,
    loaded: Vec<u64>,
    intervals: Vec<Value>,
    eof: Vec<Value>,
}
static TRACE: LazyLock<Mutex<Trace>> = LazyLock::new(|| Mutex::new(Trace::default()));

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_TONE_GEOMETRY_PROOF").is_some()
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() || path != "/api/song/enhance/player/url/v1" {
        return None;
    }
    Some((|| {
        let ids: Vec<Value> = serde_json::from_str(args["ids"].as_str().unwrap_or_default())?;
        let mut trace = TRACE.lock().unwrap();
        let &(expected, url, ..) = SOURCES
            .get(trace.source_calls.len())
            .ok_or(Error::Response(
                "unexpected repeated tone source authorization",
            ))?;
        let id = ids
            .first()
            .and_then(|id| id.as_u64().or_else(|| id.as_str()?.parse().ok()));
        if ids.len() != 1
            || id != Some(expected)
            || args["level"] != "standard"
            || args["trialMode"] != -1
        {
            return Err(Error::Response(
                "unapproved sealed tone source authorization",
            ));
        }
        trace.source_calls.push(expected);
        Ok(
            json!({"code":200,"data":[{"id":expected,"url":url,"code":200,
            "md5":"fixture","level":"standard","expi":120}]}),
        )
    })())
}

fn witness() -> &'static Witness {
    WITNESS
        .get()
        .expect("sealed tone witness already installed")
}
fn cursor() -> u64 {
    // SAFETY: the exact feature-built library remains loaded for this process.
    unsafe { (witness().cursor)() }
}

pub fn prepare(root: &Path, library: &Path) -> Result<[PathBuf; 2]> {
    let invalid = || Error::State("tone-geometry proof requires the explicit instrumented library");
    // SAFETY: the dedicated harness supplies the exact trusted fixture binary,
    // not an account response or renderer value; both functions use fixed ABI.
    let witness = unsafe {
        let library = Library::new(library).map_err(|_| invalid())?;
        Witness {
            cursor: *library
                .get::<Cursor>(b"cumulus_tone_geometry_cursor_v1\0")
                .map_err(|_| invalid())?,
            snapshot: *library
                .get::<Snapshot>(b"cumulus_tone_geometry_snapshot_v1\0")
                .map_err(|_| invalid())?,
            _library: library,
        }
    };
    WITNESS.set(witness).map_err(|_| invalid())?;
    if cursor() != 0 {
        return Err(Error::State("tone witness must start in a fresh process"));
    }
    let paths = [root.join("tone-mono.wav"), root.join("tone-stereo.wav")];
    for (path, &(_, _, rate, channels, frames)) in paths.iter().zip(&SOURCES) {
        let mut out = BufWriter::new(OpenOptions::new().write(true).create_new(true).open(path)?);
        let size = frames * u32::from(channels) * 2;
        out.write_all(b"RIFF")?;
        out.write_all(&(size + 36).to_le_bytes())?;
        out.write_all(b"WAVEfmt \x10\0\0\0\x01\0")?;
        out.write_all(&channels.to_le_bytes())?;
        out.write_all(&rate.to_le_bytes())?;
        out.write_all(&(rate * u32::from(channels) * 2).to_le_bytes())?;
        out.write_all(&(channels * 2).to_le_bytes())?;
        out.write_all(b"\x10\0data")?;
        out.write_all(&size.to_le_bytes())?;
        for frame in 0..frames {
            for channel in 0..channels {
                let sample = ((((frame * 29 + u32::from(channel) * 37) % 113) as i16) - 56) * 96;
                out.write_all(&sample.to_le_bytes())?;
            }
        }
        out.flush()?;
    }
    Ok(paths)
}

#[derive(Clone, Copy)]
struct Entry {
    song: u64,
    native: i64,
}

pub struct Audio {
    engine: MpvEngine,
    paths: [PathBuf; 2],
    entry: Option<Entry>,
    observe_format: Option<&'static str>,
}
impl Audio {
    pub fn open(paths: [PathBuf; 2], library: PathBuf) -> Result<Self> {
        let engine = MpvEngine::open(MpvConfig {
            null_output: true,
            ..Default::default()
        })?
        .with_effects_library(library);
        let mut trace = TRACE.lock().unwrap();
        if trace.opened {
            return Err(Error::State("sealed tone proof accepts one native engine"));
        }
        trace.opened = true;
        Ok(Self {
            engine,
            paths,
            entry: None,
            observe_format: None,
        })
    }

    fn mark(&self, kind: &str, before: u64, detail: Value) -> Result<()> {
        let mut trace = TRACE.lock().unwrap();
        if trace.intervals.len() == 48 {
            return Err(Error::State("too many sealed tone observations"));
        }
        trace.intervals.push(json!({"kind":kind,"song":self.entry.map(|e|e.song),
            "nativeEntry":self.entry.map(|e|e.native),"before":before,"after":cursor(),"detail":detail}));
        Ok(())
    }
}

impl AudioEngine for Audio {
    fn load(&mut self, mut request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        let Some(index) = SOURCES.iter().position(|&(_, url, ..)| {
            matches!(&request.source, MediaInput::Url(value) if value.as_str() == url)
        }) else {
            return Err(Error::InvalidInput("sealed tone engine refused nonfixture media"));
        };
        let (song, _, _, _, _) = SOURCES[index];
        {
            let trace = TRACE.lock().unwrap();
            if self.entry.is_some()
                || trace.loaded.len() != index
                || !trace.source_calls.contains(&song)
            {
                return Err(Error::State("unexpected or overlapping sealed tone load"));
            }
        }
        let before = cursor();
        // The actor performs its normal URL authorization first. Only its two
        // exact synthetic outputs are replaced here, immediately before mpv.
        request.source = MediaInput::File(self.paths[index].clone());
        let media = self.engine.load(request, cancel)?;
        self.entry = Some(Entry {
            song,
            native: media.entry_id,
        });
        self.observe_format = Some("load_format");
        TRACE.lock().unwrap().loaded.push(song);
        self.mark(
            "load",
            before,
            json!({"codec":media.codec,"duration":media.duration}),
        )?;
        Ok(media)
    }

    fn apply_equalizer(&mut self, settings: EqualizerSettings) -> Result<bool> {
        let before = cursor();
        let applied = self.engine.apply_equalizer(settings)?;
        if applied {
            let stats = self.engine.equalizer_stats()?;
            self.mark(
                "material",
                before,
                json!({"settings":settings.effective(),"revision":stats.revision}),
            )?;
        }
        Ok(applied)
    }

    fn observe_equalizer(&mut self) -> Result<Option<EqualizerStats>> {
        let stats = self.engine.observe_equalizer()?;
        TRACE.lock().unwrap().stats = stats;
        Ok(stats)
    }

    fn clear_equalizer(&mut self) -> Result<()> {
        self.engine.clear_equalizer()?;
        TRACE.lock().unwrap().stats = None;
        Ok(())
    }

    fn pause(&mut self, paused: bool) -> Result<()> {
        let before = cursor();
        self.engine.pause(paused)?;
        if self.entry.is_some() {
            self.mark(if paused { "pause" } else { "resume" }, before, json!(null))?;
        }
        Ok(())
    }

    fn seek(&mut self, seconds: f64) -> Result<()> {
        let before = cursor();
        self.engine.seek(seconds)?;
        self.observe_format = Some("seek_format");
        self.mark("seek", before, json!({"seconds":seconds}))
    }

    fn volume(&mut self, value: f64) -> Result<()> {
        self.engine.volume(value)
    }
    fn speed(&mut self, value: f64) -> Result<()> {
        self.engine.speed(value)
    }

    fn stop(&mut self) -> Result<()> {
        let before = cursor();
        self.engine.stop()?;
        if self.entry.is_some() {
            self.mark("stop", before, json!(null))?;
        }
        self.entry = None;
        self.observe_format = None;
        Ok(())
    }

    fn poll(&mut self) -> Result<EngineSnapshot> {
        let view = self.engine.poll()?;
        if let Some(entry) = self.entry {
            let now = cursor();
            if let Some(kind) = self.observe_format
                && !view.seeking
                && view.entry_id == Some(entry.native)
                && view.audio_output.as_deref() == Some("null")
                && let Some(format) = self.engine.decoded_audio_format(entry.native)?
            {
                self.mark(
                    kind,
                    now,
                    json!({"rate":format.sample_rate,"channels":format.channels,
                    "position":view.position,"paused":view.paused,"output":"null"}),
                )?;
                self.observe_format = None;
            }
            for event in &view.events {
                if let EngineEvent::Ended {
                    entry_id,
                    reason: EndReason::Eof,
                    error: 0,
                } = event
                    && *entry_id == entry.native
                {
                    let mut trace = TRACE.lock().unwrap();
                    if trace.eof.len() == SOURCES.len() {
                        return Err(Error::State("unexpected repeat sealed tone EOF"));
                    }
                    trace
                        .eof
                        .push(json!({"song":entry.song,"nativeEntry":entry.native,"cursor":now}));
                }
            }
        }
        Ok(view)
    }
}

pub fn report() -> Value {
    let mut rows = vec![[0_u64; FIELDS]; CAP];
    // SAFETY: fixed ABI and a bounded writable array of complete u64 rows.
    let count = unsafe { (witness().snapshot)(rows.as_mut_ptr().cast(), rows.len()) };
    if count < 0 || count as usize > CAP {
        return json!({"error":"native callback trace overflow or invalid snapshot","cursor":cursor()});
    }
    rows.truncate(count as usize);
    let trace = TRACE.lock().unwrap();
    json!({"oneEngine":trace.opened,"equalizerStats":trace.stats,
        "sourceCalls":trace.source_calls,"loads":trace.loaded,"intervals":trace.intervals,
        "eof":trace.eof,"callbackRows":rows,"snapshotCursor":cursor()})
}
