//! Observe real libmpv EOF events without changing what the production actor receives.
use super::relay_fixture;
use cumulus_core::{
    Error, Result,
    playback::{
        PlaybackMode,
        engine::{
            AudioEngine, Cancellation, EndReason, EngineEvent, EngineSnapshot, LoadRequest,
            LoadedMedia, MediaInput, MpvConfig, MpvEngine,
        },
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub type Trace = Arc<Mutex<Vec<Value>>>;
pub const FIRST_SECONDS: u32 = 8;
pub const TARGET_SECONDS: u32 = 30;
const FIRST: &str = "https://m1.music.126.net/cumulus-eof-fixture-first.wav";
const TARGET: &str = "https://m1.music.126.net/cumulus-eof-fixture-target.wav";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Case {
    Skip,
    Control,
    DeniedTail,
    OrderLoopSkip,
    RandomSkip,
    RandomControl,
}

impl Case {
    pub fn from_env() -> Option<Self> {
        match std::env::var("CUMULUS_ORDINARY_EOF_CASE").as_deref() {
            Ok("t1-skip") => Some(Self::Skip),
            Ok("control") => Some(Self::Control),
            Ok("t1-tail") => Some(Self::DeniedTail),
            Ok("order-loop-skip") => Some(Self::OrderLoopSkip),
            Ok("random-skip") => Some(Self::RandomSkip),
            Ok("random-control") => Some(Self::RandomControl),
            Ok(_) => panic!("unknown sealed EOF case"),
            Err(std::env::VarError::NotPresent) => None,
            Err(error) => panic!("invalid EOF case: {error}"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Skip => "t1-skip",
            Self::Control => "control",
            Self::DeniedTail => "t1-tail",
            Self::OrderLoopSkip => "order-loop-skip",
            Self::RandomSkip => "random-skip",
            Self::RandomControl => "random-control",
        }
    }

    pub fn group(self) -> &'static str {
        if matches!(self, Self::Control | Self::RandomControl) {
            "c"
        } else {
            "t1"
        }
    }

    pub fn mode(self) -> PlaybackMode {
        match self {
            Self::OrderLoopSkip => PlaybackMode::OrderLoop,
            Self::RandomSkip | Self::RandomControl => PlaybackMode::Random,
            _ => PlaybackMode::Sequential,
        }
    }

    pub fn displayed_rows(self) -> [u64; 3] {
        match self {
            Self::OrderLoopSkip => [2, 3, 1],
            Self::RandomSkip => [1, 3, 2],
            _ => [1, 2, 3],
        }
    }

    pub fn random_deck(self) -> Option<[usize; 3]> {
        // Both random cases use [3,1,2]. In treatment, the different display
        // order would authorize 3 directly. In control, 2 is denied in both
        // the random deck and the sequential display order after mode change.
        match self {
            Self::RandomSkip => Some([1, 0, 2]),
            Self::RandomControl => Some([2, 0, 1]),
            _ => None,
        }
    }

    pub fn playable(self, id: u64) -> bool {
        id == 1 || id == 3 && self != Self::DeniedTail
    }

    pub fn url(self, id: &str) -> Option<&'static str> {
        match id {
            "1" => Some(FIRST),
            "3" if self != Self::DeniedTail => Some(TARGET),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct Media {
    first: PathBuf,
    target: PathBuf,
}
impl Media {
    pub fn generate(root: &Path) -> Result<Self> {
        let first = root.join("eof-first");
        let target = root.join("eof-target");
        std::fs::create_dir(&first)?;
        std::fs::create_dir(&target)?;
        Ok(Self {
            first: relay_fixture::wave_seconds(&first, FIRST_SECONDS)?,
            target: relay_fixture::wave_seconds(&target, TARGET_SECONDS)?,
        })
    }
}

pub struct ObservedAudio {
    engine: MpvEngine,
    media: Media,
    trace: Trace,
    store: Arc<StateStore>,
    entry: Option<i64>,
    output_entry: Option<i64>,
}
impl ObservedAudio {
    pub fn open(media: Media, trace: Trace, store: Arc<StateStore>) -> Result<Self> {
        Ok(Self {
            engine: MpvEngine::open(MpvConfig {
                null_output: true,
                ..Default::default()
            })?,
            media,
            trace,
            store,
            entry: None,
            output_entry: None,
        })
    }
}
impl AudioEngine for ObservedAudio {
    fn load(&mut self, mut request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        let (id, path) = match &request.source {
            MediaInput::Url(url) if url.as_str() == FIRST => ("1", &self.media.first),
            MediaInput::Url(url) if url.as_str() == TARGET => ("3", &self.media.target),
            _ => return Err(Error::InvalidInput("EOF proof refused non-fixture media")),
        };
        request.source = MediaInput::File(path.clone());
        let checkpoint = self.store.get("2", "playback", "position")?;
        let receipt = self.engine.load(request, cancel)?;
        self.entry = Some(receipt.entry_id);
        self.trace.lock().unwrap().push(json!({
            "kind":"native_load","id":id,"entry":receipt.entry_id,"duration":receipt.duration,
            "positionBeforeLoad":checkpoint
        }));
        Ok(receipt)
    }

    fn pause(&mut self, paused: bool) -> Result<()> {
        self.engine.pause(paused)
    }
    fn seek(&mut self, _: f64) -> Result<()> {
        Err(Error::State("EOF proof cannot seek"))
    }
    fn volume(&mut self, volume: f64) -> Result<()> {
        self.engine.volume(volume)
    }
    fn speed(&mut self, speed: f64) -> Result<()> {
        self.engine.speed(speed)
    }

    fn stop(&mut self) -> Result<()> {
        self.engine.stop()?;
        if let Some(entry) = self.entry.take() {
            self.trace
                .lock()
                .unwrap()
                .push(json!({"kind":"native_stop","entry":entry}));
        }
        Ok(())
    }

    fn poll(&mut self) -> Result<EngineSnapshot> {
        let snapshot = self.engine.poll()?;
        if let Some(entry) = snapshot.entry_id
            && self.entry == Some(entry)
            && self.output_entry != Some(entry)
            && let Some(output) = &snapshot.audio_output
        {
            self.output_entry = Some(entry);
            self.trace.lock().unwrap().push(json!({
                "kind":"native_output","entry":entry,"output":output
            }));
        }
        for event in &snapshot.events {
            if let EngineEvent::Ended {
                entry_id,
                reason: EndReason::Eof,
                error,
            } = event
            {
                self.trace.lock().unwrap().push(json!({
                    "kind":"native_eof","entry":entry_id,"error":error
                }));
            }
        }
        Ok(snapshot)
    }
}
