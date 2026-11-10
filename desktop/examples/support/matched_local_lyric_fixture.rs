//! Closed CUE fixture: only the exact generated local audio can reach libmpv.
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    library::LocalLibrary,
    playback::engine::{
        AudioEngine, Cancellation, EngineSnapshot, LoadRequest, LoadedMedia, MediaInput, MpvConfig,
        MpvEngine,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

pub const SONG_ID: &str = "123";
const MATCHED_TITLE: &str = "合成本地已匹配";

#[derive(Clone)]
pub struct Fixture {
    pub directory: PathBuf,
    path: PathBuf,
    loads: Arc<AtomicUsize>,
    rejected: Arc<AtomicUsize>,
}

impl Fixture {
    pub fn new(root: &Path) -> Result<Self> {
        let directory = root.join("matched-cue-media");
        fs::create_dir(&directory)?;
        let path = directory.join("album.wav");
        fs::write(&path, super::native_lyric_wave::native_stereo_wave(12))?;
        fs::write(
            directory.join("album.cue"),
            format!(
                "FILE \"album.wav\" WAVE\nTRACK 01 AUDIO\nTITLE \"合成本地未匹配\"\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nTITLE \"{MATCHED_TITLE}\"\nINDEX 01 00:04:00\n"
            ),
        )?;
        fs::write(
            directory.join("album.lrc"),
            "[00:00]原文件旁挂第一句\n[00:02]原文件旁挂第二句\n[00:06]不应出现在已匹配分轨",
        )?;
        Ok(Self {
            directory,
            path: path.canonicalize()?,
            loads: Arc::default(),
            rejected: Arc::default(),
        })
    }

    pub fn link(&self, store: Arc<StateStore>, session: &AccountSession) -> Result<()> {
        let library = LocalLibrary::new(store)?;
        let item = library
            .list(Default::default())?
            .into_iter()
            .find(|item| item.metadata.title == MATCHED_TITLE && item.path == self.path)
            .ok_or(Error::State(
                "second CUE entry missing from synthetic index",
            ))?;
        library.map_online(session, &item.id, &json!(SONG_ID))
    }

    pub fn audio(&self) -> Result<Box<dyn AudioEngine>> {
        Ok(Box::new(ExactLocalAudio {
            fixture: self.clone(),
            engine: MpvEngine::open(MpvConfig {
                null_output: true,
                ..Default::default()
            })?,
        }))
    }

    pub fn receipt(&self) -> Value {
        json!({
            "exactLocalLoads": self.loads.load(Ordering::Acquire),
            "otherLoads": self.rejected.load(Ordering::Acquire),
        })
    }
}

struct ExactLocalAudio {
    fixture: Fixture,
    engine: MpvEngine,
}

impl AudioEngine for ExactLocalAudio {
    fn load(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        if !matches!(&request.source, MediaInput::File(path) if path.canonicalize().ok().as_ref() == Some(&self.fixture.path))
        {
            self.fixture.rejected.fetch_add(1, Ordering::AcqRel);
            return Err(Error::State(
                "synthetic CUE proof refused another audio source",
            ));
        }
        let loaded = self.engine.load(request, cancel)?;
        self.fixture.loads.fetch_add(1, Ordering::AcqRel);
        Ok(loaded)
    }

    fn pause(&mut self, paused: bool) -> Result<()> {
        self.engine.pause(paused)
    }

    fn seek(&mut self, position: f64) -> Result<()> {
        self.engine.seek(position)
    }

    fn volume(&mut self, volume: f64) -> Result<()> {
        self.engine.volume(volume)
    }

    fn speed(&mut self, speed: f64) -> Result<()> {
        self.engine.speed(speed)
    }

    fn stop(&mut self) -> Result<()> {
        self.engine.stop()
    }

    fn poll(&mut self) -> Result<EngineSnapshot> {
        self.engine.poll()
    }
}
