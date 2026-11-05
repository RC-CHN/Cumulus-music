//! Shared synthetic publication plus deny-all cold native transport.
#[path = "../../../core/examples/support/cold_offline_seed.rs"]
mod seed;
#[cfg(feature = "custom-protocol")]
use cumulus_core::{
    Error, Result,
    download::transfer::{MediaBackend, MediaReply, MediaRequest},
    protocol::{
        api::ApiFuture,
        http::{HttpBackend, HttpReply, RequestPlan},
    },
};
pub use seed::{SeedFormat, TITLE, seed};
#[cfg(feature = "custom-protocol")]
use serde_json::{Value, json};
#[cfg(feature = "custom-protocol")]
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
#[cfg(feature = "custom-protocol")]
const ACCOUNT: &str = "/eapi/w/nuser/account/get";

#[cfg(feature = "custom-protocol")]
#[derive(Default)]
pub struct Offline {
    account: AtomicUsize,
    other: AtomicUsize,
    media: AtomicUsize,
    pinned: AtomicUsize,
    pinned_ncm: AtomicUsize,
    ordinary_loads: AtomicUsize,
    observed_null: AtomicBool,
    pinned_codec: OnceLock<&'static str>,
}
#[cfg(feature = "custom-protocol")]
impl Offline {
    pub fn facts(&self) -> Value {
        json!({
            "accountAttempts":self.account.load(Ordering::Acquire),
            "otherHttpAttempts":self.other.load(Ordering::Acquire),
            "mediaAttempts":self.media.load(Ordering::Acquire),
            "pinnedLoads":self.pinned.load(Ordering::Acquire),
            "pinnedNcmLoads":self.pinned_ncm.load(Ordering::Acquire),
            "ordinaryLoads":self.ordinary_loads.load(Ordering::Acquire),
            "observedNullAudio":self.observed_null.load(Ordering::Acquire),
            "pinnedCodec":self.pinned_codec.get(),
        })
    }
}
#[cfg(feature = "custom-protocol")]
impl HttpBackend for Offline {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let counter = if plan.url.path() == ACCOUNT {
                &self.account
            } else {
                &self.other
            };
            counter.fetch_add(1, Ordering::AcqRel);
            Err(Error::Transport(
                "cold-offline proof forbids every HTTP request".into(),
            ))
        })
    }
}
#[cfg(feature = "custom-protocol")]
impl MediaBackend for Offline {
    fn get(&self, _: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            self.media.fetch_add(1, Ordering::AcqRel);
            Err(Error::Transport(
                "cold-offline proof forbids every media request".into(),
            ))
        })
    }
}

#[cfg(feature = "custom-protocol")]
mod cold_audio {
    use super::*;
    use cumulus_core::playback::engine::*;

    pub struct Audio {
        engine: MpvEngine,
        facts: Arc<Offline>,
    }

    impl Audio {
        fn record_codec(&self, loaded: &LoadedMedia) -> Result<()> {
            let codec = match loaded.codec.as_deref() {
                Some("flac") => "flac",
                Some("aac") => "aac",
                _ => return Err(Error::State("unexpected native fixture codec")),
            };
            self.facts
                .pinned_codec
                .set(codec)
                .map_err(|_| Error::State("fixture already decoded"))?;
            Ok(())
        }

        pub fn open(facts: Arc<Offline>) -> Result<Self> {
            Ok(Self {
                engine: MpvEngine::open(MpvConfig {
                    null_output: true,
                    ..Default::default()
                })?,
                facts,
            })
        }
    }

    impl AudioEngine for Audio {
        fn load(&mut self, _: LoadRequest, _: &Cancellation) -> Result<LoadedMedia> {
            self.facts.ordinary_loads.fetch_add(1, Ordering::AcqRel);
            Err(Error::State(
                "cold-offline proof requires a pinned audio handle",
            ))
        }

        fn load_pinned_audio(
            &mut self,
            file: std::fs::File,
            authority: Cancellation,
            cancel: &Cancellation,
        ) -> Result<LoadedMedia> {
            let loaded = self.engine.load_pinned_audio(file, authority, cancel)?;
            self.record_codec(&loaded)?;
            self.facts.pinned.fetch_add(1, Ordering::AcqRel);
            Ok(loaded)
        }

        fn load_pinned_ncm(
            &mut self,
            file: std::fs::File,
            authority: Cancellation,
            cancel: &Cancellation,
        ) -> Result<LoadedMedia> {
            let loaded = self.engine.load_pinned_ncm(file, authority, cancel)?;
            self.record_codec(&loaded)?;
            self.facts.pinned_ncm.fetch_add(1, Ordering::AcqRel);
            Ok(loaded)
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
            if value.audio_output.as_deref() == Some("null") {
                self.facts.observed_null.store(true, Ordering::Release);
            }
            Ok(value)
        }
    }
}
#[cfg(feature = "custom-protocol")]
pub use cold_audio::Audio;
