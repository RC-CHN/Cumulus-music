//! The test-only material selection uses the unchanged production held-source
//! worker, reader and libmpv sink. Neither a route nor a lease is synthesized.
use super::{
    Cancellation, NativeStereoPass, NativeStereoSelection, NativeStereoSource, NativeStereoStart,
};
use crate::{
    Error,
    effects::{
        equalizer::EqualizerSettings, material::ModernLimiterPlan,
        stereo::LocalEqualizerMaterialPlan,
    },
    library::{identity::fingerprint, native_open::checked_audio_file},
    playback::engine::{
        AudioEngine, EndReason, EngineEvent, MpvConfig, MpvEngine, NativeModernEffectSource,
        StereoPcmControl, StereoPcmSnapshot, limiter_segment::native_decoder::until,
        pcm_wave::tests::wave_at, stereo_pcm::StereoGraphControl,
    },
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::mpsc::sync_channel,
    time::{Duration, Instant},
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

fn pause_modern_entry(
    native: &mut MpvEngine,
    control: &StereoPcmControl,
    entry: i64,
    stage: &str,
) -> StereoPcmSnapshot {
    native.pause(true).unwrap();
    let mut stable: Option<(StereoPcmSnapshot, Instant)> = None;
    actual(stage, || {
        let view = native.poll().unwrap();
        let state = control.snapshot().unwrap();
        assert_eq!(view.entry_id, Some(entry));
        assert_eq!(state.native_entry, Some(entry));
        assert!(!state.failed && !state.cancelled && !state.graph_end);
        if !view.paused || !state.paused || state.seeking {
            stable = None;
            return None;
        }
        if let Some((previous, since)) = stable
            && previous == state
        {
            // Let any already-credited decoder/native callback finish
            // before establishing the graph-control boundary.
            return (since.elapsed() >= Duration::from_millis(80)).then_some(state);
        }
        stable = Some((state, Instant::now()));
        None
    })
}

fn switch_paused_modern_entry(
    control: &StereoPcmControl,
    before: StereoPcmSnapshot,
    active: bool,
) -> u64 {
    assert!(before.delivered_frames > before.native_output_frame);
    let revision = control
        .queue_graph_control(StereoGraphControl::SetModernMaterialActive(active))
        .unwrap();
    assert_eq!(revision, before.requested_revision + 1);
    control
        .wait_applied(revision, Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        control.snapshot().unwrap(),
        StereoPcmSnapshot {
            requested_revision: revision,
            applied_revision: revision,
            ..before
        },
        "the worker changes only the graph while the old native entry and PCM stay put"
    );
    revision
}

#[test]
fn held_geq_modern_admission_is_read_only_and_stamped_to_the_worker_revision() {
    let rate = 48_000;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("admission.wav");
    fs::write(&path, wave_at(rate, (0..32 * 2048 + 7).map(s16))).unwrap();
    let path = path.canonicalize().unwrap();
    let fingerprint = fingerprint(&path).unwrap();
    let mut gains = [0.0; 10];
    gains[1] = 3.0;
    let geq = EqualizerSettings::new(true, gains).unwrap();
    let local = |settings| {
        let plan = LocalEqualizerMaterialPlan::from_saved(settings);
        let (file, _) = checked_audio_file(&path, &fingerprint).unwrap();
        let authority = Cancellation::default();
        let NativeStereoSelection::Qualified(source) =
            NativeStereoSource::select_wave(file, authority.clone(), plan).unwrap()
        else {
            panic!("must hold the eligible WAV descriptor");
        };
        let pass = start(*source, 0, &Cancellation::default());
        let control = pass.control();
        until(|| (control.snapshot().unwrap().produced_frames > 0).then_some(()));
        control.publish_sink().unwrap();
        control.bind_entry(71).unwrap();
        control.observe(71, Some(0.0), true, false).unwrap();
        (pass, control, authority)
    };

    let (pass, control, lifetime) = local(geq);
    let first = NativeModernEffectSource::capture(control.clone(), 71)
        .unwrap()
        .unwrap();
    assert_eq!(control.modern_admission_revision(71).unwrap(), Some(0));
    let plan = LocalEqualizerMaterialPlan::from_saved(geq);
    let rev = pass.apply_material(plan).unwrap();
    assert!(
        matches!(first.check(), Err(Error::StaleResponse)),
        "any queued revision stales the old receipt even when the plan is unchanged"
    );
    control.wait_applied(rev, Duration::from_secs(2)).unwrap();
    let steady = control.snapshot().unwrap();
    let latest = NativeModernEffectSource::capture(control.clone(), 71)
        .unwrap()
        .unwrap();
    assert_eq!(control.modern_admission_revision(71).unwrap(), Some(rev));
    assert!(
        NativeModernEffectSource::capture(control.clone(), 72)
            .unwrap()
            .is_none()
    );
    latest.check().unwrap();
    assert_eq!(
        control.snapshot().unwrap(),
        steady,
        "reading the graph stamp must not touch PCM, clock, or revision"
    );

    let off = pass.disable_master().unwrap();
    assert!(matches!(latest.check(), Err(Error::StaleResponse)));
    control.wait_applied(off, Duration::from_secs(2)).unwrap();
    let reapply = pass.apply_material(plan).unwrap();
    control
        .wait_applied(reapply, Duration::from_secs(2))
        .unwrap();
    assert!(
        NativeModernEffectSource::capture(control.clone(), 71)
            .unwrap()
            .is_none()
    );
    control.retire_sink();
    let _source = pass.reclaim().unwrap();
    assert!(!lifetime.is_cancelled());

    for settings in [
        geq.with_tone(3, 0).unwrap(),
        geq.with_stereo_controls(3, 0).unwrap(),
    ] {
        let (pass, control, _) = local(settings);
        assert!(
            NativeModernEffectSource::capture(control.clone(), 71)
                .unwrap()
                .is_none()
        );
        control.retire_sink();
        let _source = pass.reclaim().unwrap();
    }

    let (fresh_pass, fresh, _) = local(geq);
    assert_eq!(fresh.modern_admission_revision(71).unwrap(), Some(0));
    assert!(
        first.check().is_err(),
        "a new pass can reuse numeric entry and revision without reviving the old receipt"
    );
    fresh.retire_sink();
    let _source = fresh_pass.reclaim().unwrap();
}

#[test]
#[ignore = "requires compatible libmpv; runs a same-entry local-to-modern handoff with null output"]
fn initial_modern_uses_same_held_local_worker_and_native_entry_until_short_eof() {
    let plan = ModernLimiterPlan::from_material(&vector()["profiles"]["neutralTone"]).unwrap();
    let mut gains = [0.0; 10];
    gains[1] = 3.0;
    let old = LocalEqualizerMaterialPlan::from_saved(EqualizerSettings::new(true, gains).unwrap());
    let rate = 48_000;
    let total = 72 * 2048u64 + 7;
    let old_debt = 238;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("local-geq.wav");
    fs::write(&path, wave_at(rate, (0..total as usize).map(s16))).unwrap();
    let path = path.canonicalize().unwrap();
    let (file, format) = checked_audio_file(&path, &fingerprint(&path).unwrap()).unwrap();
    assert_eq!(format, "wav");
    let lifetime = Cancellation::default();
    let NativeStereoSelection::Qualified(source) =
        NativeStereoSource::select_wave(file, lifetime.clone(), old).unwrap()
    else {
        panic!("the normal local graph must qualify the held WAV");
    };
    fs::rename(&path, dir.path().join("same-descriptor.wav")).unwrap();
    fs::write(&path, b"the replacement name must not be opened").unwrap();

    let mut native = MpvEngine::open(MpvConfig {
        library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
        null_output: true,
    })
    .unwrap();
    let request = Cancellation::default();
    let mut pass = start(*source, 0, &request);
    let control = pass.control();
    let loaded = native
        .load_stereo_pcm(pass.take_reader().unwrap(), &request)
        .unwrap();
    assert_eq!(loaded.codec.as_deref(), Some("pcm_f32le"));
    assert!(loaded.duration.is_none());
    let entry = loaded.entry_id;
    native.pause(false).unwrap();
    actual("existing local GEQ entry is playing", || {
        let view = native.poll().unwrap();
        (view.audio_output.as_deref() == Some("null")
            && view.position_for(entry).is_some_and(|p| p > 0.2))
        .then_some(())
    });
    let before = pause_modern_entry(
        &mut native,
        &control,
        entry,
        "pause old local entry in place",
    );
    assert!(before.delivered_frames > before.native_output_frame);
    assert_eq!(
        before.decoded_source_frame - before.produced_frames,
        old_debt
    );
    for expect_success in [true, false] {
        let (outcome, receipt) = sync_channel(1);
        let revision = control
            .queue_graph_control(StereoGraphControl::InitialModernFromLocal {
                plan: Box::new(plan.clone()),
                outcome,
            })
            .unwrap();
        assert_eq!(
            receipt
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .is_ok(),
            expect_success
        );
        control
            .wait_applied(revision, Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            control.snapshot().unwrap(),
            StereoPcmSnapshot {
                requested_revision: revision,
                applied_revision: revision,
                ..before
            },
            "a graph switch or rejected duplicate must leave the decoder, FIFO, and native clock untouched"
        );
    }
    assert_eq!(native.poll().unwrap().entry_id, Some(entry));

    // Off then on before another decoder quantum: the final setup reaches
    // the same live graph without consuming a separate off block.
    let paused = control.snapshot().unwrap();
    assert_eq!(switch_paused_modern_entry(&control, paused, false), 3);
    let paused = control.snapshot().unwrap();
    assert_eq!(switch_paused_modern_entry(&control, paused, true), 4);
    let advance = |native: &mut MpvEngine, previous: StereoPcmSnapshot, label| {
        native.pause(false).unwrap();
        actual(label, || {
            let view = native.poll().unwrap();
            let state = control.snapshot().unwrap();
            assert!(!state.failed && !state.cancelled);
            (view.entry_id == Some(entry)
                && state.decoded_source_frame >= previous.decoded_source_frame + 2048
                && state.native_output_frame >= previous.native_output_frame + u64::from(rate) / 5)
                .then_some(())
        });
        pause_modern_entry(native, &control, entry, label)
    };
    let modern_debt = 191;
    let active = advance(
        &mut native,
        before,
        "the coalesced first modern quantum ran",
    );
    assert_eq!(
        active.decoded_source_frame - active.produced_frames,
        old_debt + modern_debt
    );
    assert_eq!(switch_paused_modern_entry(&control, active, false), 5);
    let off = advance(
        &mut native,
        active,
        "a real off quantum ran on the same native entry",
    );
    let debt_after_reapply = 2 * old_debt + modern_debt;
    assert_eq!(
        off.decoded_source_frame - off.produced_frames,
        debt_after_reapply
    );
    assert_eq!(switch_paused_modern_entry(&control, off, true), 6);
    native.pause(false).unwrap();
    actual(
        "the same entry drains the modern graph's short final quantum",
        || {
            let view = native.poll().unwrap();
            assert!(view.events.iter().all(|event| !matches!(event,
            EngineEvent::Ended {entry_id, ..} if *entry_id != entry)));
            view.events.iter().any(|event| matches!(event,
            EngineEvent::Ended {entry_id, reason: EndReason::Eof, error: 0} if *entry_id == entry
        )).then_some(())
        },
    );
    let end = actual("that entry and reader retire only at natural EOF", || {
        let state = control.snapshot().unwrap();
        (state.reader_closed && state.native_sink_retired).then_some(state)
    });
    assert!(end.graph_end && end.cancelled && !end.failed, "{end:?}");
    assert!(!lifetime.is_cancelled());
    assert_eq!(end.decoded_source_frame, total);
    assert_eq!(end.produced_frames, total - debt_after_reapply);
    assert_eq!(end.delivered_frames, end.produced_frames);
    assert_eq!(end.applied_revision, 6);
    assert!(end.native_source_frame > off.native_source_frame);
    assert!(end.native_source_frame <= end.delivered_frames);
    assert_eq!(pass.reclaim().unwrap().total_frames(), total);
    eprintln!(
        "held local-to-modern receipt: before={before:?}, active={active:?}, off={off:?}, end={end:?}"
    );
}

#[test]
#[ignore = "requires compatible libmpv; exercises the real paused native sink and shortened EOF"]
fn modern_held_wave_native_sink_applies_live_controls_and_shortens_eof() {
    let vectors = vector();
    let mut profile = vectors["profiles"]["nonzeroTone"].clone();
    // Match the release-only timing variation of the frozen hot corpus.
    // The two real switches keep attack/geometry constant, but each
    // discards 238 frames of old B0 lookahead at 48 kHz.
    profile["aev2"]["limiter"]["attack"] = serde_json::json!(0.005);
    let plan = ModernLimiterPlan::from_material(&profile).unwrap();
    let rate = 48_000;
    let quantum = 2048u64;
    let total = 72 * quantum + 7;
    let seek_frame = 54 * quantum;
    let old_debt = 238u64;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hot.wav");
    let wave = wave_at(rate, (0..total as usize).map(s16));
    let lifetime = Cancellation::default();
    let source = hold(&path, &wave, lifetime.clone(), &plan);
    fs::rename(&path, dir.path().join("still-owned.wav")).unwrap();
    fs::write(&path, b"do not reopen the changed pathname").unwrap();

    let mut native = MpvEngine::open(MpvConfig {
        library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
        null_output: true,
    })
    .unwrap();
    let request = Cancellation::default();
    let mut pass = start(source, 0, &request);
    let control = pass.control();
    let loaded = native
        .load_stereo_pcm(pass.take_reader().unwrap(), &request)
        .unwrap();
    assert_eq!(loaded.codec.as_deref(), Some("pcm_f32le"));
    assert!(loaded.duration.is_none());
    let entry = loaded.entry_id;
    native.pause(false).unwrap();
    actual(
        "active entry clocks its first real PCM before the off control",
        || {
            let view = native.poll().unwrap();
            (view.audio_output.as_deref() == Some("null")
                && view.position_for(entry).is_some_and(|p| p > 0.2))
            .then_some(())
        },
    );
    let active = pause_modern_entry(&mut native, &control, entry, "settled pause before off");
    assert_eq!(
        active.decoded_source_frame - active.produced_frames,
        old_debt
    );
    assert_eq!(switch_paused_modern_entry(&control, active, false), 1);
    assert!(
        native.seek(0.5).is_err(),
        "the raw entry cannot reopen on seek"
    );
    native.pause(false).unwrap();
    actual(
        "same entry runs at least one off quantum before reapply",
        || {
            let view = native.poll().unwrap();
            let state = control.snapshot().unwrap();
            assert!(!state.failed && !state.cancelled);
            (view.entry_id == Some(entry)
                && state.decoded_source_frame >= active.decoded_source_frame + quantum
                && state.native_output_frame >= active.native_output_frame + u64::from(rate) / 5)
                .then_some(())
        },
    );
    let off = pause_modern_entry(&mut native, &control, entry, "settled pause before reapply");
    assert_eq!(off.decoded_source_frame - off.produced_frames, old_debt * 2);
    assert_eq!(switch_paused_modern_entry(&control, off, true), 2);
    native.pause(false).unwrap();
    actual(
        "reapplied graph naturally drains a short final quantum",
        || {
            let view = native.poll().unwrap();
            view.events
                .iter()
                .any(|event| {
                    matches!(event, EngineEvent::Ended {
                entry_id, reason: EndReason::Eof, error: 0,
            } if *entry_id == entry)
                })
                .then_some(())
        },
    );
    let drained = actual("first reader and native sink retired", || {
        let state = control.snapshot().unwrap();
        (state.reader_closed && state.native_sink_retired).then_some(state)
    });
    // Poll retires the per-entry transport even after clean natural EOF;
    // that cancellation must not revoke the still-owned source descriptor.
    assert!(
        drained.graph_end && !drained.failed && drained.cancelled,
        "{drained:?}"
    );
    assert!(!lifetime.is_cancelled());
    assert_eq!(drained.decoded_source_frame, total);
    assert_eq!(drained.produced_frames, total - 2 * old_debt);
    assert_eq!(drained.delivered_frames, drained.produced_frames);
    assert_eq!(drained.applied_revision, 2);
    assert!(drained.native_source_frame > off.native_source_frame);
    assert!(drained.native_source_frame <= drained.delivered_frames);
    assert!(
        control
            .queue_graph_control(StereoGraphControl::SetModernMaterialActive(false))
            .is_err()
    );
    eprintln!("held hot receipt: active={active:?}, off={off:?}, drained={drained:?}");

    native.stop().unwrap();
    let source = pass.reclaim().unwrap();
    let mut seek = start(source, seek_frame, &request);
    let sought = seek.control();
    let reopened = native
        .load_stereo_pcm(seek.take_reader().unwrap(), &request)
        .unwrap();
    let next_entry = reopened.entry_id;
    assert_ne!(entry, next_entry);
    let fresh = sought.snapshot().unwrap();
    assert!(native.poll().unwrap().paused);
    assert_eq!(fresh.native_source_frame, seek_frame);
    assert_eq!(fresh.native_output_frame, 0);
    assert_eq!(fresh.applied_revision, 0);
    native.pause(false).unwrap();
    let mut saw_fresh_clock = false;
    actual(
        "seek entry drains from the original descriptor with no hot frame debt",
        || {
            let view = native.poll().unwrap();
            let state = sought.snapshot().unwrap();
            assert_eq!(state.first_source_frame, seek_frame);
            saw_fresh_clock |= state.native_source_frame > seek_frame;
            view.events
                .iter()
                .any(|event| {
                    matches!(event, EngineEvent::Ended {
                entry_id, reason: EndReason::Eof, error: 0,
            } if *entry_id == next_entry)
                })
                .then_some(())
        },
    );
    assert!(saw_fresh_clock);
    let end = actual("seek reader and native sink retired", || {
        let state = sought.snapshot().unwrap();
        (state.reader_closed && state.native_sink_retired).then_some(state)
    });
    assert!(end.graph_end && !end.failed && end.cancelled, "{end:?}");
    assert!(!lifetime.is_cancelled());
    assert_eq!(end.decoded_source_frame, total);
    assert_eq!(end.produced_frames, total - seek_frame);
    assert_eq!(end.delivered_frames, end.produced_frames);
    native.stop().unwrap();
    let (mut original, original_lifetime) = seek
        .reclaim()
        .unwrap()
        .into_reader()
        .unwrap()
        .into_stock()
        .unwrap();
    let mut bytes = Vec::new();
    original.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, wave);
    lifetime.cancel();
    assert!(matches!(original_lifetime.check(), Err(Error::Cancelled)));
    eprintln!("held hot seek receipt: fresh={fresh:?}, drained={end:?}");
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
