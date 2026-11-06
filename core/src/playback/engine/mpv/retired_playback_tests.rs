//! Reproduce failed native Stop acknowledgement through the production player.
use super::*;
use crate::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{PlayerAction, PlayerView},
    },
    effects::equalizer::{EqualizerSettings, EqualizerStatus},
    playback::{
        controller::PlaybackStatus,
        engine::{AudioEngine, EngineSnapshot, LoadRequest, LoadedMedia, MpvConfig},
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

struct Closed;
impl Api for Closed {
    fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async { panic!("local explicit recovery must not call the network") })
    }
}

struct ForceRejectedStop {
    inner: MpvEngine,
    arm: Arc<AtomicBool>,
    destroyed: Arc<AtomicBool>,
    reopened: Arc<AtomicUsize>,
}
impl AudioEngine for ForceRejectedStop {
    fn reopen_retired_local_sink(&mut self, cancel: &Cancellation) -> Result<bool> {
        let result = self.inner.reopen_retired_local_sink(cancel)?;
        if result {
            self.reopened.fetch_add(1, Ordering::AcqRel);
            let empty = self.inner.poll()?;
            assert!(empty.entry_id.is_none() && empty.paused && empty.events.is_empty());
        }
        Ok(result)
    }
    fn output_devices(&mut self) -> Result<Option<crate::playback::engine::OutputDevices>> {
        self.inner.output_devices()
    }
    fn set_output_device(&mut self, id: &str) -> Result<()> {
        self.inner.set_output_device(id)
    }
    fn load(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        self.inner.load(request, cancel)
    }
    fn supports_local_equalizer(&self) -> bool {
        self.inner.supports_local_equalizer()
    }
    fn load_with_local_equalizer(
        &mut self,
        request: LoadRequest,
        settings: EqualizerSettings,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        self.inner
            .load_with_local_equalizer(request, settings, cancel)
    }
    fn local_equalizer_entry(&self) -> Option<i64> {
        self.inner.local_equalizer_entry()
    }
    fn local_equalizer_processing(&mut self) -> Result<bool> {
        self.inner.local_equalizer_processing()
    }
    fn pump_local_equalizer(&mut self) -> Result<()> {
        self.inner.pump_local_equalizer()
    }
    fn apply_local_equalizer(&mut self, settings: EqualizerSettings) -> Result<()> {
        self.inner.apply_local_equalizer(settings)
    }
    fn volume(&mut self, value: f64) -> Result<()> {
        self.inner.volume(value)
    }
    fn speed(&mut self, value: f64) -> Result<()> {
        self.inner.speed(value)
    }
    fn pause(&mut self, paused: bool) -> Result<()> {
        self.inner.pause(paused)
    }
    fn seek(&mut self, value: f64) -> Result<()> {
        self.inner.seek(value)
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        self.inner.poll()
    }
    fn stop(&mut self) -> Result<()> {
        if self.arm.swap(false, Ordering::AcqRel) {
            assert!(self.inner.local_equalizer_entry().is_some());
            // Genuine libmpv command rejection. No synthetic END_FILE, no
            // direct assignment to the production native handle.
            let error = self
                .inner
                .retire_pinned_with(|engine, entry| {
                    engine.pinned_stop_command(
                        entry,
                        c"cumulus-invalid-stop-proof",
                        &Cancellation::default(),
                    )
                })
                .unwrap_err();
            assert!(matches!(error, Error::Playback { operation: "command", code } if code < 0));
            assert!(matches!(
                self.inner.poll(),
                Err(Error::Playback {
                    operation: "native sink retired",
                    code: -1
                })
            ));
            self.destroyed.store(true, Ordering::Release);
        }
        self.inner.stop()
    }
}

fn wave() -> Vec<u8> {
    const RATE: u32 = 44_100;
    const FRAMES: u32 = RATE * 3 + 7;
    let data = FRAMES * 4;
    let mut bytes = Vec::with_capacity(data as usize + 44);
    bytes.extend(b"RIFF");
    bytes.extend((36 + data).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend([1, 0, 2, 0]);
    bytes.extend(RATE.to_le_bytes());
    bytes.extend((RATE * 4).to_le_bytes());
    bytes.extend([4, 0, 16, 0]);
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    for i in 0..FRAMES {
        let sample = ((i * 97) % 8192) as i16 - 4096;
        bytes.extend(sample.to_le_bytes());
        bytes.extend((sample / 4).to_le_bytes());
    }
    bytes
}

async fn playing(client: &MusicClient, epoch: &str, id: &str) -> PlayerView {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let view = client.player_snapshot(epoch).await.unwrap();
            assert!(view.error.is_none(), "{:?}", view.error);
            if view.state == PlaybackStatus::Playing
                && view.play_id.as_deref() == Some(id)
                && view.equalizer.status == EqualizerStatus::Processing
                && view.position.is_some_and(|position| position > 0.12)
            {
                return view;
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    })
    .await
    .expect("actual native processed clock")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires libmpv and native file inspection; uses synthetic WAV and null output"]
async fn retired_playback_requires_a_fresh_explicit_native_file_action() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(&dir.path().join("state"), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(
        Arc::new(Closed),
        Some(registry.store()),
    ));
    let arm = Arc::new(AtomicBool::new(false));
    let destroyed = Arc::new(AtomicBool::new(false));
    let reopened = Arc::new(AtomicUsize::new(0));
    let client = MusicClient::with_session(session, registry).with_audio_factory({
        let arm = arm.clone();
        let destroyed = destroyed.clone();
        let reopened = reopened.clone();
        move || {
            Ok(Box::new(ForceRejectedStop {
                inner: MpvEngine::open(MpvConfig {
                    library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
                    null_output: true,
                })?
                .with_local_equalizer_pcm_wave(),
                arm: arm.clone(),
                destroyed: destroyed.clone(),
                reopened: reopened.clone(),
            }))
        }
    });
    let epoch = client.account().unwrap().epoch;
    let current = client.player_snapshot(&epoch).await.unwrap();
    let settings =
        EqualizerSettings::new(true, [0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]).unwrap();
    client
        .player_action(
            &epoch,
            PlayerAction::Equalizer {
                expected_revision: current.equalizer.revision,
                settings,
                preset: None,
            },
        )
        .await
        .unwrap();
    let first = dir.path().join("first.wav");
    let second = dir.path().join("second.wav");
    std::fs::write(&first, wave()).unwrap();
    std::fs::write(&second, wave()).unwrap();
    let first = client
        .play_native_open_files(&epoch, vec![first])
        .await
        .unwrap();
    playing(&client, &epoch, first.play_id.as_deref().unwrap()).await;
    arm.store(true, Ordering::Release);
    let stopped = client
        .player_action(&epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(destroyed.load(Ordering::Acquire));
    assert!(stopped.play_id.is_none() && stopped.state == PlaybackStatus::Idle);
    let current = client.player_snapshot(&epoch).await.unwrap();
    assert!(current.play_id.is_none() && current.state == PlaybackStatus::Idle);
    assert_eq!(
        reopened.load(Ordering::Acquire),
        0,
        "Stop or observation reopened playback"
    );
    assert!(
        client
            .play_native_open_files("999", vec![second.clone()])
            .await
            .is_err()
    );
    assert!(
        client
            .play_native_open_files(&epoch, vec![dir.path().join("missing.wav")])
            .await
            .is_err()
    );
    assert_eq!(
        reopened.load(Ordering::Acquire),
        0,
        "stale or missing selection opened a sink"
    );
    let next = client
        .play_native_open_files(&epoch, vec![second])
        .await
        .expect("new local file after fail-closed native retirement");
    assert_ne!(next.play_id, first.play_id);
    assert_ne!(next.entry_id, first.entry_id);
    assert_eq!(reopened.load(Ordering::Acquire), 1);
    playing(&client, &epoch, next.play_id.as_deref().unwrap()).await;
}
