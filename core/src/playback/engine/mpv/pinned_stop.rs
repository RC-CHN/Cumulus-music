//! A revoked pinned source must leave no live native sink with queued PCM.
use super::{Cancellation, EndReason, EngineEvent, Error, MpvEngine, Received, Result};
use std::{
    ffi::CStr,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_millis(750);

fn stop_observation(entry: i64, event: &EngineEvent) -> Option<bool> {
    match event {
        EngineEvent::Ended {
            entry_id,
            reason,
            error,
        } if *entry_id == entry => Some(*reason == EndReason::Stop && *error == 0),
        EngineEvent::Shutdown => Some(false),
        _ => None,
    }
}

fn no_stop_ack() -> Error {
    Error::Playback {
        operation: "pinned stop was not acknowledged",
        code: -1,
    }
}

impl MpvEngine {
    pub(super) fn retire_pinned(&mut self) {
        self.retire_pinned_with(|engine, entry| engine.pinned_stop_command(entry, c"stop"));
    }

    fn retire_pinned_with(
        &mut self,
        acknowledge: impl FnOnce(&mut Self, i64) -> Result<EngineEvent>,
    ) {
        let stereo = self.stereo_pcm.take();
        if let Some(control) = &stereo {
            control.cancel();
        }
        if let Some(gate) = self.pinned_authority.take() {
            gate.cancel();
        }
        if let Some(native) = &self.native {
            native.clear_staged_pinned();
        }
        let stop = self
            .entry
            .take()
            .and_then(|entry| acknowledge(self, entry).ok());
        self.events.clear();
        if let Some(stop) = stop {
            self.events.push_back(stop);
        } else {
            // The C API is synchronous: after Native::drop completes, its
            // renderer, stream callbacks, decoder and AO no longer exist.
            // Do not keep a failed handle alive for an attempted retry.
            drop(self.native.take());
            self.pinned_bmff_options = false;
        }
        if let Some(control) = stereo {
            control.retire_sink();
        }
    }

    fn pinned_stop_command(&mut self, entry: i64, command: &CStr) -> Result<EngineEvent> {
        let deadline = Instant::now() + WAIT;
        self.command(vec![command.to_owned()], deadline, &Cancellation::default())?;
        self.await_pinned_stop(entry, deadline)
    }

    fn await_pinned_stop(&mut self, entry: i64, deadline: Instant) -> Result<EngineEvent> {
        // A command reply only acknowledges the command, not the end of
        // audio. Wait for the exact active entry's successful END_FILE Stop.
        if self
            .events
            .iter()
            .any(|event| matches!(event, EngineEvent::Shutdown))
        {
            return Err(no_stop_ack());
        }
        if let Some((event, matched)) = self
            .events
            .iter()
            .find_map(|event| stop_observation(entry, event).map(|matched| (event, matched)))
        {
            return if matched {
                Ok(event.clone())
            } else {
                Err(no_stop_ack())
            };
        }
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(no_stop_ack());
            }
            if let Received::Event(event) = self
                .native_mut()?
                .event(remaining.as_secs_f64().min(0.05))?
            {
                match stop_observation(entry, &event) {
                    Some(true) => return Ok(event),
                    Some(false) => return Err(no_stop_ack()),
                    None => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::engine::{AudioEngine, MpvConfig};
    use std::{
        io::{self, Cursor, Read, Seek, SeekFrom},
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };

    #[test]
    fn pinned_stop_only_accepts_the_current_entry_and_actual_stop_reason() {
        let ended = |entry_id, reason, error| EngineEvent::Ended {
            entry_id,
            reason,
            error,
        };
        assert_eq!(stop_observation(8, &ended(7, EndReason::Stop, 0)), None);
        assert_eq!(
            stop_observation(8, &ended(8, EndReason::Eof, 0)),
            Some(false)
        );
        assert_eq!(
            stop_observation(8, &ended(8, EndReason::Stop, -1)),
            Some(false)
        );
        assert_eq!(
            stop_observation(8, &ended(8, EndReason::Stop, 0)),
            Some(true)
        );
        assert_eq!(stop_observation(8, &EngineEvent::Shutdown), Some(false));
    }

    struct HeldReader {
        bytes: Cursor<&'static [u8]>,
        closed: Arc<AtomicBool>,
        revoke_on_read: Option<Cancellation>,
    }
    impl Read for HeldReader {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if let Some(authority) = self.revoke_on_read.take() {
                authority.cancel();
            }
            self.bytes.read(out)
        }
    }
    impl Seek for HeldReader {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.bytes.seek(position)
        }
    }
    impl Drop for HeldReader {
        fn drop(&mut self) {
            self.closed.store(true, Ordering::Release);
        }
    }

    #[test]
    #[ignore = "requires compatible native libmpv; exercises real held-reader destruction"]
    fn pinned_stop_native_rejection_and_stale_ack_both_destroy_the_sink() {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/tag_audio/seed.flac"
        ));
        for reject in [true, false] {
            let closed = Arc::new(AtomicBool::new(false));
            let mut engine = MpvEngine::open(MpvConfig {
                library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
                null_output: true,
            })
            .unwrap();
            let authority = Cancellation::default();
            let loaded = engine
                .load_pinned(
                    Box::new(HeldReader {
                        bytes: Cursor::new(bytes.as_slice()),
                        closed: closed.clone(),
                        revoke_on_read: None,
                    }),
                    "flac",
                    authority.clone(),
                    &Cancellation::default(),
                )
                .unwrap();
            assert!(!closed.load(Ordering::Acquire));
            authority.cancel();
            engine
                .retain(EngineEvent::Ended {
                    entry_id: loaded.entry_id + 1,
                    reason: EndReason::Stop,
                    error: 0,
                })
                .unwrap();
            let mut attempted = false;
            engine.retire_pinned_with(|engine, entry| {
                assert_eq!(entry, loaded.entry_id);
                attempted = true;
                let result = if reject {
                    // Force an actual native command rejection with the
                    // pinned reader still owned by libmpv, before teardown.
                    engine.pinned_stop_command(entry, c"cumulus-invalid-stop-probe")
                } else {
                    // No real Stop was sent. Only the old wrong-entry event
                    // is available; it cannot stand in for a native ack.
                    engine.await_pinned_stop(entry, Instant::now() + Duration::from_millis(35))
                };
                if reject {
                    assert!(matches!(&result, Err(Error::Playback { operation: "command", code }) if *code < 0));
                } else {
                    assert!(matches!(&result, Err(Error::Playback { operation: "pinned stop was not acknowledged", .. })));
                }
                result
            });
            assert!(attempted);
            assert!(closed.load(Ordering::Acquire));
            assert!(engine.native.is_none());
            assert!(engine.entry.is_none() && engine.events.is_empty());
            assert!(matches!(
                engine.poll(),
                Err(Error::Playback {
                    operation: "native sink retired",
                    ..
                })
            ));
            assert!(engine.stop().is_ok());
            eprintln!(
                "native pinned Stop {}; held reader closed synchronously, no old events published",
                if reject {
                    "command rejected"
                } else {
                    "ack missing with stale event"
                }
            );
        }
    }

    #[test]
    #[ignore = "requires compatible native libmpv; exercises cancellation during real input open"]
    fn pinned_stop_revocation_while_opening_closes_the_unpublished_sink() {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/tag_audio/seed.flac"
        ));
        let closed = Arc::new(AtomicBool::new(false));
        let mut engine = MpvEngine::open(MpvConfig {
            library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
            null_output: true,
        })
        .unwrap();
        let authority = Cancellation::default();
        let result = engine.load_pinned(
            Box::new(HeldReader {
                bytes: Cursor::new(bytes.as_slice()),
                closed: closed.clone(),
                revoke_on_read: Some(authority.clone()),
            }),
            "flac",
            authority,
            &Cancellation::default(),
        );
        assert!(matches!(result, Err(Error::Cancelled)));
        assert!(closed.load(Ordering::Acquire));
        assert!(engine.entry.is_none());
        if engine.native.is_some() {
            // Some demuxers assign an internal entry even though the load
            // is never returned. Reuse is allowed only with its real Stop.
            assert_eq!(engine.events.len(), 1);
            assert!(matches!(
                engine.events.front(),
                Some(EngineEvent::Ended {
                    reason: EndReason::Stop,
                    error: 0,
                    ..
                })
            ));
        } else {
            assert!(engine.events.is_empty());
        }
        eprintln!(
            "native pinned input revoked during first read: no load published; reader closed via {}",
            if engine.native.is_some() {
                "native Stop"
            } else {
                "sink destruction"
            }
        );
    }
}
