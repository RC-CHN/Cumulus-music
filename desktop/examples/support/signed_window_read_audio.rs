//! Reuse the production worker, libmpv pinned transport and native clock.
use super::signed_window_read_gate::Gate;
#[cfg(feature = "signed-window-read-proof")]
use super::signed_window_read_gate::MediaArm;
use cumulus_core::{
    Error, Result,
    download::transfer::{MediaBackend, MediaUrl},
    playback::engine::{
        AudioEngine, Cancellation, EngineSnapshot, LoadRequest, LoadedMedia, MediaInput, MpvConfig,
        MpvEngine,
    },
};
use std::{
    os::unix::fs::MetadataExt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

pub enum Expectation {
    Synthetic {
        bytes: u64,
    },
    #[cfg(feature = "signed-window-read-proof")]
    Live,
}

impl Expectation {
    fn accepts_duration(&self, value: f64) -> bool {
        match self {
            Self::Synthetic { .. } => (11.8..12.3).contains(&value),
            #[cfg(feature = "signed-window-read-proof")]
            Self::Live => (1.0..=3600.0).contains(&value),
        }
    }
}

#[derive(Default)]
pub struct NativeFacts {
    pub pinned_loads: AtomicUsize,
    pub null_clock: AtomicUsize,
}

pub struct Audio {
    engine: MpvEngine,
    facts: Arc<NativeFacts>,
    gate: Arc<Gate>,
    media: Arc<dyn MediaBackend>,
    directory: PathBuf,
    expectation: Expectation,
    #[cfg(feature = "signed-window-read-proof")]
    arm: Option<Arc<dyn MediaArm>>,
}

impl Audio {
    pub fn open(
        gate: Arc<Gate>,
        media: Arc<dyn MediaBackend>,
        facts: Arc<NativeFacts>,
        directory: PathBuf,
        expectation: Expectation,
    ) -> Result<Self> {
        Ok(Self {
            engine: MpvEngine::open(MpvConfig {
                null_output: true,
                ..Default::default()
            })?,
            facts,
            gate,
            media,
            directory,
            expectation,
            #[cfg(feature = "signed-window-read-proof")]
            arm: None,
        })
    }

    #[cfg(feature = "signed-window-read-proof")]
    pub fn with_media_arm(mut self, arm: Arc<dyn MediaArm>) -> Self {
        self.arm = Some(arm);
        self
    }

    async fn fetch(&self, url: &MediaUrl, cancel: &Cancellation) -> Result<std::fs::File> {
        #[cfg(feature = "signed-window-read-proof")]
        if let Some(arm) = &self.arm {
            return self
                .gate
                .fetch_verified_armed(&*self.media, url, &self.directory, cancel, Some(&**arm))
                .await;
        }
        self.gate
            .fetch_verified(&*self.media, url, &self.directory, cancel)
            .await
    }
}

impl AudioEngine for Audio {
    fn load(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        let MediaInput::Url(url) = request.source else {
            return Err(Error::State("signed proof refused non-URL transport input"));
        };
        if request.start != 0.0 || request.end.is_some() {
            return Err(Error::State("signed proof requires an ordinary full track"));
        }
        let url = MediaUrl::parse(url.as_str())?;
        // The production controller calls AudioEngine on spawn_blocking.
        let file = tauri::async_runtime::block_on(self.fetch(&url, cancel))?;
        let metadata = file.metadata()?;
        if metadata.nlink() != 0
            || metadata.mode() & 0o777 != 0o600
            || metadata.len() == 0
            || matches!(self.expectation, Expectation::Synthetic { bytes } if metadata.len() != bytes)
        {
            return Err(Error::State(
                "verified AAC was not held in a private anonymous file",
            ));
        }
        let opened = self
            .engine
            .load_pinned_audio(file, cancel.clone(), cancel)?;
        if opened.codec.as_deref() != Some("aac")
            || !opened
                .duration
                .is_some_and(|value| self.expectation.accepts_duration(value))
        {
            return Err(Error::State("unexpected real AAC decode"));
        }
        self.facts.pinned_loads.fetch_add(1, Ordering::AcqRel);
        Ok(opened)
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
        let value = self.engine.poll()?;
        if value.audio_output.as_deref() == Some("null")
            && value.position.is_some_and(|value| value > 0.1)
        {
            self.facts.null_clock.fetch_add(1, Ordering::Relaxed);
        }
        Ok(value)
    }
}
