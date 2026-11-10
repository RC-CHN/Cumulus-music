//! The test-only material selection uses the unchanged production held-source
//! worker, reader and libmpv sink. Neither a route nor a lease is synthesized.
use super::{
    Cancellation, NativeStereoPass, NativeStereoSelection, NativeStereoSource, NativeStereoStart,
};
use crate::{
    Error,
    effects::material::ModernLimiterPlan,
    library::{identity::fingerprint, native_open::checked_audio_file},
    playback::engine::{
        AudioEngine, EndReason, EngineEvent, MpvConfig, MpvEngine,
        limiter_segment::native_decoder::until, pcm_wave::tests::wave_at,
    },
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

fn vector() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/effects-modern-limiter-graph-official.json"
    )))
    .unwrap()
}

// Exact integer encoding of the PCM in effects_modern_limiter_original.rs;
// WAV s16 to normalized f32 is lossless for these dyadic values.
fn s16(i: usize) -> [i16; 2] {
    [
        if i.is_multiple_of(27) {
            28_672
        } else {
            (((i * 37) % 256) as i16 - 128) * 64
        },
        if i.is_multiple_of(29) {
            -20_480
        } else {
            (((i * 53) % 128) as i16 - 64) * 64
        },
    ]
}

fn hold(
    path: &Path,
    wav: &[u8],
    authority: Cancellation,
    plan: &ModernLimiterPlan,
) -> NativeStereoSource {
    fs::write(path, wav).unwrap();
    let path = path.canonicalize().unwrap();
    let (file, format) = checked_audio_file(&path, &fingerprint(&path).unwrap()).unwrap();
    assert_eq!(format, "wav");
    let NativeStereoSelection::Qualified(source) =
        NativeStereoSource::select_modern_wave_test(file, authority, plan).unwrap()
    else {
        panic!("the verified PCM WAV must qualify for the modern test graph");
    };
    *source
}

fn start(source: NativeStereoSource, first: u64, request: &Cancellation) -> NativeStereoPass {
    let NativeStereoStart::Ready(pass) = source.start(first, request).unwrap() else {
        panic!("the already-held WAV must prepare without a path reopen");
    };
    *pass
}

fn compare(bytes: &[u8], expected: &Value) {
    assert_eq!(
        bytes.len() as u64,
        expected["outputFrames"].as_u64().unwrap() * 8
    );
    assert_eq!(
        hex::encode(Sha256::digest(bytes)),
        expected["outputSha256"].as_str().unwrap()
    );
}

// The regular test uses a deterministic consuming sink for byte comparison;
// the opt-in test below separately uses the real libmpv native clock.
fn consume(pass: &mut NativeStereoPass, rate: u32, entry: i64) -> Vec<u8> {
    let control = pass.control();
    control.publish_sink().unwrap();
    control.bind_entry(entry).unwrap();
    control.observe(entry, Some(0.0), false, false).unwrap();
    let mut reader = pass.take_reader().unwrap();
    let mut output = Vec::new();
    let mut window = [0; 4093];
    loop {
        until(|| {
            let state = control.snapshot().unwrap();
            assert!(!state.failed && !state.cancelled);
            (state.produced_frames > state.delivered_frames || state.graph_end).then_some(())
        });
        let read = reader.read(&mut window).unwrap();
        if read == 0 {
            break;
        }
        output.extend_from_slice(&window[..read]);
        control
            .observe(
                entry,
                Some((output.len() / 8) as f64 / f64::from(rate)),
                false,
                false,
            )
            .unwrap();
    }
    assert_eq!(reader.read(&mut window).unwrap(), 0, "stable producer EOF");
    drop(reader);
    control.retire_sink();
    let state = control.snapshot().unwrap();
    assert!(state.graph_end && state.reader_closed && state.native_sink_retired && !state.failed);
    assert_eq!(state.decoded_source_frame, state.source_end);
    assert_eq!(state.produced_frames, state.delivered_frames);
    assert_eq!(state.produced_frames, (output.len() / 8) as u64);
    output
}

#[test]
fn modern_held_wave_worker_matches_original_pcm_seek_and_exact_eof() {
    let vectors = vector();
    let dir = tempfile::tempdir().unwrap();
    let request = Cancellation::default();
    for (i, case) in vectors["cases"].as_array().unwrap().iter().enumerate() {
        let rate = case["rate"].as_u64().unwrap() as u32;
        let plan = ModernLimiterPlan::from_material(
            &vectors["profiles"][case["profile"].as_str().unwrap()],
        )
        .unwrap();
        let fresh = wave_at(rate, (0..6151).map(s16));
        let source = hold(
            &dir.path().join(format!("fresh-{i}.wav")),
            &fresh,
            Cancellation::default(),
            &plan,
        );
        let mut first = start(source, 0, &request);
        compare(&consume(&mut first, rate, 21), &case["split"]);

        // The first decoder quantum is identical to the original irregular
        // input. The rest of this file has a 4096-frame suffix matching the
        // original reset vector, so the reset must append exactly one sentinel.
        let path = dir.path().join(format!("owned-{i}.wav"));
        let seek_frame = 2055;
        let expected = wave_at(rate, (0..seek_frame).chain(10_000..14_096).map(s16));
        let lifetime = Cancellation::default();
        let source = hold(&path, &expected, lifetime.clone(), &plan);
        fs::rename(&path, dir.path().join(format!("moved-{i}.wav"))).unwrap();
        fs::write(&path, b"a replacement name is not the held WAV").unwrap();
        let mut old = start(source, 0, &request);
        let old_control = old.control();
        old_control.publish_sink().unwrap();
        old_control.bind_entry(22).unwrap();
        old_control.observe(22, Some(0.0), false, false).unwrap();
        let mut old_reader = old.take_reader().unwrap();
        let prefix = &case["quanta"][0];
        let first_output = prefix["outputFrames"].as_u64().unwrap();
        until(|| {
            let state = old_control.snapshot().unwrap();
            assert!(!state.failed && !state.cancelled);
            (state.produced_frames >= first_output).then_some(())
        });
        old_control.observe(22, Some(0.0), true, false).unwrap();
        let mut pcm = vec![0; first_output as usize * 8];
        old_reader.read_exact(&mut pcm).unwrap();
        compare(&pcm, prefix);
        drop(old_reader);
        let released = old_control.snapshot().unwrap();
        assert!(released.reader_closed && !released.native_sink_retired);
        // A released reader alone cannot hand the still-published sink's
        // source to a new pass. Only explicit sink retirement allows reclaim.
        old_control.retire_sink();
        assert!(!lifetime.is_cancelled());
        let source = old.reclaim().unwrap();
        let mut seek = start(source, seek_frame as u64, &request);
        assert_eq!(
            seek.control().snapshot().unwrap().first_source_frame,
            seek_frame as u64
        );
        compare(&consume(&mut seek, rate, 23), &case["resetAfterEof"]);
        assert_eq!(seek.control().snapshot().unwrap().delivered_frames, 4097);
        let (mut file, exact_lifetime) = seek
            .reclaim()
            .unwrap()
            .into_reader()
            .unwrap()
            .into_stock()
            .unwrap();
        let mut reopened = Vec::new();
        file.read_to_end(&mut reopened).unwrap();
        assert_eq!(
            reopened, expected,
            "the seek returned the original descriptor's bytes"
        );
        lifetime.cancel();
        assert!(matches!(exact_lifetime.check(), Err(Error::Cancelled)));
    }
}

fn actual<T>(stage: &str, poll: impl FnMut() -> Option<T>) -> T {
    eprintln!("held modern limiter: {stage}");
    until(poll)
}

#[test]
#[ignore = "requires compatible libmpv; uses real held WAV/native clocks and null output"]
fn modern_held_wave_native_sink_seek_eof_and_revocation() {
    let vectors = vector();
    let plan = ModernLimiterPlan::from_material(&vectors["profiles"]["nonzeroTone"]).unwrap();
    let rate = 48_000u32;
    let total = u64::from(rate) * 2 + 7;
    let seek_frame = u64::from(rate) / 2;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("native.wav");
    let held = Cancellation::default();
    let prepare = Cancellation::default();
    let wave = wave_at(rate, (0..total as usize).map(s16));
    let source = hold(&path, &wave, held.clone(), &plan);
    fs::rename(&path, dir.path().join("renamed.wav")).unwrap();
    fs::write(&path, b"cannot replace the held WAV").unwrap();
    let mut native = MpvEngine::open(MpvConfig {
        library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
        null_output: true,
    })
    .unwrap();
    let mut first = start(source, 0, &prepare);
    let old_control = first.control();
    let old = native
        .load_stereo_pcm(first.take_reader().unwrap(), &prepare)
        .unwrap();
    assert_eq!(old.codec.as_deref(), Some("pcm_f32le"));
    assert!(
        old.duration.is_none(),
        "raw sink has no invented container duration"
    );
    let initial = native.poll().unwrap();
    assert!(initial.paused && initial.entry_id == Some(old.entry_id));
    assert_eq!(old_control.snapshot().unwrap().native_output_frame, 0);
    prepare.cancel();
    native.pause(false).unwrap();
    actual("first entry uses the real null-output clock", || {
        let view = native.poll().unwrap();
        (view.audio_output.as_deref() == Some("null")
            && view
                .position_for(old.entry_id)
                .is_some_and(|position| position > 0.12))
        .then_some(())
    });
    native.pause(true).unwrap();
    assert!(
        native.seek(0.5).is_err(),
        "raw callbacks do not support a user seek"
    );
    native
        .test_retire_held_stereo(old.entry_id, &Cancellation::default())
        .unwrap();
    actual("old sink retired and reader closed", || {
        let state = old_control.snapshot().unwrap();
        (state.reader_closed && state.native_sink_retired).then_some(())
    });
    let source = first.reclaim().unwrap();
    let request = Cancellation::default();
    let mut second = start(source, seek_frame, &request);
    let new_control = second.control();
    let new = native
        .load_stereo_pcm(second.take_reader().unwrap(), &request)
        .unwrap();
    assert_ne!(new.entry_id, old.entry_id);
    assert!(native.poll().unwrap().paused);
    assert_eq!(
        new_control.snapshot().unwrap().first_source_frame,
        seek_frame
    );
    native.pause(false).unwrap();
    let mut progressed = false;
    actual(
        "same descriptor's seek entry naturally reaches verified EOF",
        || {
            let view = native.poll().unwrap();
            let state = new_control.snapshot().unwrap();
            assert!(state.native_source_frame >= seek_frame && state.native_source_frame <= total);
            progressed |= view
                .position_for(new.entry_id)
                .is_some_and(|position| position > 0.15);
            view.events
                .iter()
                .any(|event| {
                    matches!(event, EngineEvent::Ended {
            entry_id, reason: EndReason::Eof, error: 0,
        } if *entry_id == new.entry_id)
                })
                .then_some(())
        },
    );
    assert!(progressed);
    let state = new_control.snapshot().unwrap();
    assert!(state.graph_end && state.native_sink_retired && !state.failed);
    assert_eq!(state.decoded_source_frame, total);
    assert_eq!(
        (state.produced_frames, state.delivered_frames),
        (total - seek_frame, total - seek_frame)
    );
    actual("naturally ended reader closed", || {
        new_control.snapshot().unwrap().reader_closed.then_some(())
    });
    native.stop().unwrap();
    let source = second.reclaim().unwrap();
    let mut third = start(source, 0, &request);
    let revoked = third.control();
    let current = native
        .load_stereo_pcm(third.take_reader().unwrap(), &request)
        .unwrap();
    assert_ne!(current.entry_id, new.entry_id);
    native.pause(false).unwrap();
    actual(
        "revocable entry has real decoded PCM and native clock",
        || {
            let view = native.poll().unwrap();
            let state = revoked.snapshot().unwrap();
            (!state.graph_end
                && state.delivered_frames > 0
                && view
                    .position_for(current.entry_id)
                    .is_some_and(|position| position > 0.08))
            .then_some(())
        },
    );
    held.cancel();
    assert!(
        matches!(native.poll(), Err(Error::Cancelled)),
        "source revocation is not normal EOF"
    );
    actual("revoked native sink retired", || {
        let state = revoked.snapshot().unwrap();
        (state.reader_closed && state.native_sink_retired).then_some(())
    });
    assert!(
        third.reclaim().is_err(),
        "revocation forbids opening any replacement from the old descriptor"
    );
}
