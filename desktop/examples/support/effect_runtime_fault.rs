//! Probe-only adapter: inject typed processor faults after a real native Pause.
use cumulus_core::{
    Error, Result,
    effects::{
        equalizer::{EqualizerSettings, abi::EqualizerStats},
        loudness::{LoudnessSettings, abi::LoudnessStats},
    },
    playback::engine::{
        AudioEngine, Cancellation, EngineSnapshot, LoadRequest, LoadedMedia, MpvConfig, MpvEngine,
    },
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub struct Audio {
    engine: MpvEngine,
    armed: Arc<AtomicBool>,
    pending: u8,
}

impl Audio {
    pub fn open(path: PathBuf, armed: Arc<AtomicBool>) -> Result<Self> {
        Ok(Self {
            engine: MpvEngine::open(MpvConfig {
                null_output: true,
                ..Default::default()
            })?
            .with_effects_library(path),
            armed,
            pending: 0,
        })
    }
}

impl AudioEngine for Audio {
    fn load(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        self.engine.load(request, cancel)
    }

    fn pause(&mut self, paused: bool) -> Result<()> {
        self.engine.pause(paused)?;
        if paused && self.armed.swap(false, Ordering::AcqRel) {
            self.pending = 2;
        }
        Ok(())
    }

    fn poll(&mut self) -> Result<EngineSnapshot> {
        match self.pending {
            2 => {
                self.pending -= 1;
                Err(Error::EqualizerProcessorFailed)
            }
            1 => {
                self.pending -= 1;
                Err(Error::LoudnessProcessorFailed)
            }
            _ => self.engine.poll(),
        }
    }

    fn apply_equalizer(&mut self, settings: EqualizerSettings) -> Result<bool> {
        self.engine.apply_equalizer(settings)
    }

    fn clear_equalizer(&mut self) -> Result<()> {
        self.engine.clear_equalizer()?;
        if self.engine.observe_equalizer()?.is_some() {
            return Err(Error::State("GEQ filter still attached after clear"));
        }
        Ok(())
    }

    fn observe_equalizer(&mut self) -> Result<Option<EqualizerStats>> {
        self.engine.observe_equalizer()
    }

    fn apply_loudness(&mut self, settings: LoudnessSettings) -> Result<bool> {
        self.engine.apply_loudness(settings)
    }

    fn clear_loudness(&mut self) -> Result<()> {
        self.engine.clear_loudness()?;
        if self.engine.observe_loudness()?.is_some() {
            return Err(Error::State("loudness filter still attached after clear"));
        }
        Ok(())
    }

    fn observe_loudness(&mut self) -> Result<Option<LoudnessStats>> {
        self.engine.observe_loudness()
    }

    fn seek(&mut self, value: f64) -> Result<()> {
        self.engine.seek(value)
    }

    fn speed(&mut self, value: f64) -> Result<()> {
        self.engine.speed(value)
    }

    fn volume(&mut self, value: f64) -> Result<()> {
        self.engine.volume(value)
    }

    fn stop(&mut self) -> Result<()> {
        self.engine.stop()
    }
}
