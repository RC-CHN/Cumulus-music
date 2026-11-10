//! Negative control for a proposed stop-and-reload modern graph handoff.
//! The fixture uses the production held WAV decoder and raw libmpv entry.
use super::*;
use crate::{
    effects::equalizer::EqualizerSettings,
    library::identity::fingerprint,
    playback::engine::{AudioEngine, LoadRequest, MediaInput, MpvConfig, pcm_wave::tests::wave_at},
};
use std::{fs, path::PathBuf};

#[test]
#[ignore = "requires compatible libmpv; tests rejected exact Stop on a real held WAV"]
fn modern_handoff_cannot_roll_back_a_missing_or_rejected_exact_stop() {
    const RATE: u32 = 48_000;
    let bytes = wave_at(
        RATE,
        (0..(RATE * 2 + 7)).map(|index| {
            let sample = ((index * 97) % 8192) as i16 - 4096;
            [sample, -sample / 4]
        }),
    );
    for stale_event in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("original.wav");
        fs::write(&path, &bytes).unwrap();
        let path = path.canonicalize().unwrap();
        let request = LoadRequest::new(MediaInput::NativeFile {
            fingerprint: fingerprint(&path).unwrap(),
            path: path.clone(),
        });
        let settings =
            EqualizerSettings::new(true, [0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
                .unwrap();
        let mut engine = MpvEngine::open(MpvConfig {
            library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
            null_output: true,
        })
        .unwrap()
        .with_local_equalizer_pcm_wave();
        let action = Cancellation::default();
        let loaded = engine
            .load_with_local_equalizer(request, settings, &action)
            .unwrap();
        let entry = loaded.entry_id;
        assert_eq!(loaded.codec.as_deref(), Some("pcm_f32le"));
        assert_eq!(engine.local_equalizer_entry(), Some(entry));
        fs::rename(&path, temp.path().join("held.wav")).unwrap();
        fs::write(path, b"not the held source").unwrap();
        engine.pause(false).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let view = engine.poll().unwrap();
            if view.audio_output.as_deref() == Some("null")
                && view.position_for(entry).is_some_and(|pos| pos > 0.12)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "owned WAV never obtained a real native clock"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        engine.pause(true).unwrap();
        let receipt = engine.modern_effect_owned_source(entry).unwrap().unwrap();
        let old = engine.stereo_pcm.as_ref().unwrap().clone();
        let active = old.snapshot().unwrap();
        assert!(active.native_source_frame > 0 && !active.graph_end);
        assert!(active.delivered_frames > active.native_output_frame);
        assert!(matches!(
            engine.retire_local_pinned_with(entry + 1, &action, |_, _, _| panic!("wrong request")),
            Err(Error::StaleResponse)
        ));
        receipt.check().unwrap();
        if stale_event {
            engine
                .retain(EngineEvent::Ended {
                    entry_id: entry + 1,
                    reason: EndReason::Stop,
                    error: 0,
                })
                .unwrap();
        }
        let result = engine.retire_local_pinned_with(entry, &action, |engine, entry, guard| {
            assert!(guard.check().is_ok());
            assert!(matches!(receipt.check(), Err(Error::StaleResponse)));
            assert!(engine.modern_effect_owned_source(entry).unwrap().is_none());
            if stale_event {
                engine.await_pinned_stop(entry, Instant::now() + Duration::from_millis(35), guard)
            } else {
                // The real mpv command rejects; no claimed successful Stop.
                engine.pinned_stop_command(entry, c"cumulus-invalid-modern-handoff", guard)
            }
        });
        if stale_event {
            assert!(matches!(
                result,
                Err(Error::Playback {
                    operation: "pinned stop was not acknowledged",
                    ..
                })
            ));
        } else {
            assert!(
                matches!(result, Err(Error::Playback { operation: "command", code }) if code < 0)
            );
        }
        let retired = old.snapshot().unwrap();
        assert!(retired.cancelled && retired.reader_closed && retired.native_sink_retired);
        assert!(!action.is_cancelled());
        assert!(receipt.check().is_err());
        assert!(engine.native.is_none() && engine.entry.is_none() && engine.events.is_empty());
        assert!(
            engine.local_equalizer.is_none(),
            "the old descriptor owner was discarded"
        );
        assert!(matches!(
            engine.poll(),
            Err(Error::Playback {
                operation: "native sink retired",
                ..
            })
        ));

        // Reopening the native runtime after proven retirement can only
        // allocate an empty sink. It is neither rollback nor source transfer.
        assert!(engine.reopen_retired_local_sink(&action).unwrap());
        let empty = engine.poll().unwrap();
        assert!(
            empty.entry_id.is_none()
                && empty.position.is_none()
                && empty.paused
                && empty.events.is_empty()
        );
        assert!(engine.modern_effect_owned_source(entry).unwrap().is_none());
        assert!(receipt.check().is_err());
        eprintln!(
            "held WAV failed intentional handoff: stale_event={stale_event}, native_source_frame={}, delivered_frames={}, old_reader_closed={}, new_entry={:?}",
            active.native_source_frame,
            active.delivered_frames,
            retired.reader_closed,
            empty.entry_id
        );
    }
}
