//! A pinned source cannot hand over ownership until its native sink retires.
use super::{Cancellation, EndReason, EngineEvent, Error, MpvEngine, Received, Result};
use std::{
    ffi::CStr,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_millis(750);

fn stop_observation(entry: i64, event: &EngineEvent) -> Option<Result<()>> {
    match event {
        EngineEvent::Ended {
            entry_id,
            reason,
            error,
        } if *entry_id == entry => Some(match (reason, error) {
            (EndReason::Stop, 0) => Ok(()),
            _ => Err(Error::Playback {
                operation: match reason {
                    EndReason::Stop => "pinned Stop reported an error",
                    EndReason::Eof => "pinned Stop observed EOF",
                    EndReason::Quit => "pinned Stop observed quit",
                    EndReason::Error => "pinned Stop observed an input error",
                    EndReason::Redirect => "pinned Stop observed a redirect",
                    EndReason::Unknown(_) => "pinned Stop observed an unknown end reason",
                },
                code: *error,
            }),
        }),
        EngineEvent::Shutdown => Some(Err(Error::Playback {
            operation: "pinned Stop observed shutdown",
            code: -1,
        })),
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
        let _ = self.retire_pinned_with(|engine, entry| {
            engine.pinned_stop_command(entry, c"stop", &Cancellation::default())
        });
    }

    pub(super) fn retire_local_equalizer_pinned(
        &mut self,
        entry: i64,
        cancel: &Cancellation,
    ) -> Result<()> {
        if self.local_equalizer.is_none() || self.entry != Some(entry) {
            return Err(Error::StaleResponse);
        }
        self.retire_local_pinned_with(entry, cancel, |engine, entry, guard| {
            engine.pinned_stop_command(entry, c"stop", guard)
        })
    }

    fn retire_local_pinned_with(
        &mut self,
        entry: i64,
        cancel: &Cancellation,
        acknowledge: impl FnOnce(&mut Self, i64, &Cancellation) -> Result<EngineEvent>,
    ) -> Result<()> {
        if self.entry != Some(entry) {
            return Err(Error::StaleResponse);
        }
        let source = (|| {
            cancel.check()?;
            self.pinned_authority
                .as_ref()
                .ok_or(Error::State("local equalizer sink authority is missing"))?
                .check()?;
            self.stereo_pcm
                .as_ref()
                .ok_or(Error::State("local equalizer sink is missing"))?
                .local_handoff_authority(entry)
        })();
        let source = match source {
            Ok(source) => source,
            Err(error) => {
                // No intentional handoff is allowed once old authority or
                // its pass is invalid. Retire with the existing cancel-first
                // path and preserve the initiating error.
                self.retire_pinned();
                return Err(error);
            }
        };
        let request = cancel.clone();
        let guard = Cancellation::new(move || !source.is_cancelled() && !request.is_cancelled());
        // An old pass still owned by this entry may supply audio until
        // libmpv actually stops it. Its own cancel callback wakes a blocked
        // PCM reader; prematurely poisoning that read can look like EOF.
        let stop = guard
            .check()
            .and_then(|()| acknowledge(self, entry, &guard))
            .and_then(|event| {
                guard.check()?;
                stop_observation(entry, &event).ok_or_else(no_stop_ack)??;
                Ok(event)
            });
        self.cancel_pinned_input();
        self.finish_pinned_retirement(stop)
    }

    fn retire_pinned_with(
        &mut self,
        acknowledge: impl FnOnce(&mut Self, i64) -> Result<EngineEvent>,
    ) -> Result<()> {
        self.cancel_pinned_input();
        let stop = self.entry.take().ok_or_else(no_stop_ack).and_then(|entry| {
            let event = acknowledge(self, entry)?;
            stop_observation(entry, &event).ok_or_else(no_stop_ack)??;
            Ok(event)
        });
        self.finish_pinned_retirement(stop)
    }

    fn cancel_pinned_input(&self) {
        if let Some(control) = &self.stereo_pcm {
            control.cancel();
        }
        if let Some(gate) = &self.pinned_authority {
            gate.cancel();
        }
        if let Some(native) = &self.native {
            native.clear_staged_pinned();
        }
    }

    fn finish_pinned_retirement(&mut self, stop: Result<EngineEvent>) -> Result<()> {
        self.entry = None;
        self.pinned_authority = None;
        let stereo = self.stereo_pcm.take();
        self.events.clear();
        let result = match stop {
            Ok(event) => {
                self.events.push_back(event);
                Ok(())
            }
            Err(error) => {
                // After Native::drop completes, no stream callbacks, decoder or
                // audio output remain. A failed handle is never reused.
                drop(self.native.take());
                self.pinned_bmff_options = false;
                Err(error)
            }
        };
        if let Some(control) = stereo {
            control.retire_sink();
        }
        result
    }

    fn pinned_stop_command(
        &mut self,
        entry: i64,
        command: &CStr,
        guard: &Cancellation,
    ) -> Result<EngineEvent> {
        let deadline = Instant::now() + WAIT;
        self.command(vec![command.to_owned()], deadline, guard)?;
        self.await_pinned_stop(entry, deadline, guard)
    }

    fn await_pinned_stop(
        &mut self,
        entry: i64,
        deadline: Instant,
        guard: &Cancellation,
    ) -> Result<EngineEvent> {
        // A command reply only acknowledges the command, not the end of
        // audio. Wait for the exact active entry's successful END_FILE Stop.
        guard.check()?;
        if let Some(event) = self
            .events
            .iter()
            .find(|event| matches!(event, EngineEvent::Shutdown))
        {
            return stop_observation(entry, event)
                .expect("shutdown always ends the native wait")
                .map(|()| event.clone());
        }
        if let Some((event, matched)) = self
            .events
            .iter()
            .find_map(|event| stop_observation(entry, event).map(|matched| (event, matched)))
        {
            return matched.map(|()| event.clone());
        }
        loop {
            guard.check()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(no_stop_ack());
            }
            if let Received::Event(event) = self
                .native_mut()?
                .event(remaining.as_secs_f64().min(0.05))?
                && let Some(matched) = stop_observation(entry, &event)
            {
                return matched.map(|()| event);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::engine::{
        AudioEngine, MpvConfig,
        stereo_pcm::{channel, native_channel},
    };
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
        assert!(stop_observation(8, &ended(7, EndReason::Stop, 0)).is_none());
        assert!(matches!(
            stop_observation(8, &ended(8, EndReason::Eof, 0)),
            Some(Err(Error::Playback {
                operation: "pinned Stop observed EOF",
                code: 0
            }))
        ));
        assert!(matches!(
            stop_observation(8, &ended(8, EndReason::Stop, -1)),
            Some(Err(Error::Playback {
                operation: "pinned Stop reported an error",
                code: -1
            }))
        ));
        assert!(matches!(
            stop_observation(8, &ended(8, EndReason::Stop, 0)),
            Some(Ok(()))
        ));
        assert!(matches!(
            stop_observation(8, &EngineEvent::Shutdown),
            Some(Err(Error::Playback {
                operation: "pinned Stop observed shutdown",
                code: -1
            }))
        ));
    }

    fn no_native() -> MpvEngine {
        MpvEngine {
            native: None,
            equalizer: None,
            equalizer_reserved: false,
            loudness: None,
            loudness_reserved: false,
            parametric: None,
            equalizer_library: None,
            sequence: 0,
            events: Default::default(),
            entry: None,
            null_output: true,
            api_version: 0,
            pinned_authority: None,
            pinned_bmff_options: false,
            stereo_pcm: None,
            local_flac: None,
            local_wave: false,
            local_equalizer: None,
        }
    }

    #[test]
    fn ordinary_pinned_stop_keeps_authority_until_native_cancel_and_rejects_stale_entry() {
        let mut engine = no_native();
        let source = Cancellation::default();
        let (reader, control, _) = native_channel(44100, 0, 10000, source.clone()).unwrap();
        control.publish_sink().unwrap();
        control.bind_entry(8).unwrap();
        engine.entry = Some(8);
        engine.stereo_pcm = Some(control.clone());
        engine.pinned_authority = Some(control.authority());
        let request = Cancellation::default();
        assert!(matches!(
            engine.retire_local_pinned_with(7, &request, |_, _, _| panic!("stale entry")),
            Err(Error::StaleResponse)
        ));
        assert!(!control.snapshot().unwrap().cancelled);
        assert_eq!(engine.entry, Some(8));
        engine
            .retire_local_pinned_with(8, &request, |engine, entry, guard| {
                assert_eq!(entry, 8);
                assert!(!control.snapshot().unwrap().cancelled);
                assert!(engine.pinned_authority.as_ref().unwrap().check().is_ok());
                // Model libmpv's cancel and then close. Native callback
                // cancellation must wake the reader without revoking the
                // actual held source lease used while waiting for END_FILE.
                control.cancel();
                drop(reader);
                assert!(engine.pinned_authority.as_ref().unwrap().is_cancelled());
                assert!(guard.check().is_ok());
                assert!(!control.snapshot().unwrap().native_sink_retired);
                Ok(EngineEvent::Ended {
                    entry_id: entry,
                    reason: EndReason::Stop,
                    error: 0,
                })
            })
            .unwrap();
        let view = control.snapshot().unwrap();
        assert!(view.cancelled && view.reader_closed && view.native_sink_retired);
        assert!(!source.is_cancelled() && !request.is_cancelled());
        assert_eq!(engine.entry, None);
        assert_eq!(engine.events.len(), 1);
    }

    #[test]
    fn ordinary_pinned_stop_returns_non_stop_and_revoked_source_without_receipt() {
        for revoked in [false, true] {
            let mut engine = no_native();
            let source = Cancellation::default();
            let (reader, control, _) = native_channel(44100, 0, 10000, source.clone()).unwrap();
            control.publish_sink().unwrap();
            control.bind_entry(8).unwrap();
            engine.entry = Some(8);
            engine.stereo_pcm = Some(control.clone());
            engine.pinned_authority = Some(control.authority());
            let result =
                engine.retire_local_pinned_with(8, &Cancellation::default(), |_, entry, guard| {
                    assert!(guard.check().is_ok());
                    if revoked {
                        source.cancel();
                        assert!(guard.is_cancelled());
                    }
                    drop(reader);
                    Ok(EngineEvent::Ended {
                        entry_id: entry,
                        reason: if revoked {
                            EndReason::Stop
                        } else {
                            EndReason::Eof
                        },
                        error: 0,
                    })
                });
            if revoked {
                assert!(matches!(result, Err(Error::Cancelled)));
            } else {
                assert!(matches!(
                    result,
                    Err(Error::Playback {
                        operation: "pinned Stop observed EOF",
                        code: 0
                    })
                ));
            }
            assert!(engine.entry.is_none() && engine.events.is_empty());
            assert!(control.snapshot().unwrap().native_sink_retired);
        }
    }

    #[test]
    fn generic_pinned_stop_cancels_input_before_asking_for_native_ack() {
        let mut engine = no_native();
        let source = Cancellation::default();
        let (reader, control, _) = channel(44100, 0, 10000, source.clone(), None).unwrap();
        control.publish_sink().unwrap();
        control.bind_entry(8).unwrap();
        engine.entry = Some(8);
        engine.stereo_pcm = Some(control.clone());
        engine.pinned_authority = Some(control.authority());
        assert!(matches!(
            control.local_handoff_authority(8),
            Err(Error::StaleResponse)
        ));
        engine
            .retire_pinned_with(|_, entry| {
                assert!(control.snapshot().unwrap().cancelled);
                assert!(!control.snapshot().unwrap().native_sink_retired);
                drop(reader);
                Ok(EngineEvent::Ended {
                    entry_id: entry,
                    reason: EndReason::Stop,
                    error: 0,
                })
            })
            .unwrap();
        assert!(control.snapshot().unwrap().native_sink_retired);
        assert!(!source.is_cancelled());
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
                    engine.pinned_stop_command(
                        entry,
                        c"cumulus-invalid-stop-probe",
                        &Cancellation::default(),
                    )
                } else {
                    // No real Stop was sent. Only the old wrong-entry event
                    // is available; it cannot stand in for a native ack.
                    engine.await_pinned_stop(
                        entry,
                        Instant::now() + Duration::from_millis(35),
                        &Cancellation::default(),
                    )
                };
                if reject {
                    assert!(matches!(&result, Err(Error::Playback { operation: "command", code }) if *code < 0));
                } else {
                    assert!(matches!(&result, Err(Error::Playback { operation: "pinned stop was not acknowledged", .. })));
                }
                result
            }).unwrap_err();
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
