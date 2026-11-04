use super::*;
use crate::effects::{
    equalizer::{EqualizerSettings, abi::EqualizerStats},
    loudness::LoudnessSettings,
    parametric::ParametricPlan,
};
use serde_json::json;

fn wav(path: &std::path::Path) -> Result<()> {
    use std::io::{BufWriter, Write};
    let mut file = BufWriter::new(std::fs::File::create(path)?);
    let frames = 48_000_u32 * 5;
    let bytes = 4 * frames;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + bytes).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&2_u16.to_le_bytes())?;
    file.write_all(&48_000_u32.to_le_bytes())?;
    file.write_all(&192_000_u32.to_le_bytes())?;
    file.write_all(&4_u16.to_le_bytes())?;
    file.write_all(&16_u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&bytes.to_le_bytes())?;
    for frame in 0..frames {
        let sample = ((frame % 110) as i16 - 55) * 80;
        file.write_all(&sample.to_le_bytes())?;
        file.write_all(&(-sample).to_le_bytes())?;
    }
    file.flush()?;
    Ok(())
}

fn running(stats: EqualizerStats) -> bool {
    stats.active_instances == 2
        && stats.failed_blocks == 0
        && stats.processed_samples > 1024
        && stats.observed_revision == stats.revision
}

#[test]
#[ignore = "requires explicit trusted filter library and libmpv with LADSPA"]
fn native_reserved_geq_preserves_loudness_and_peq_instances() -> Result<()> {
    let library = PathBuf::from(std::env::var_os("CUMULUS_PROOF_EFFECT_FILTER").ok_or(
        Error::InvalidInput("supply trusted filter library via CUMULUS_PROOF_EFFECT_FILTER"),
    )?);
    let root = tempfile::tempdir()?;
    let media = root.path().join("stereo.wav");
    wav(&media)?;
    let plan = ParametricPlan::from_material(&json!({
        "aev2":{"peq":{"on":true,"gain":-2,"f":[{"band":1,"type":6,"freq":440,"gain":-4,"q":0.7}]}}
    }))?
    .unwrap();
    for loudness in [false, true] {
        let mut engine = MpvEngine::open(MpvConfig {
            null_output: true,
            ..Default::default()
        })?;
        engine.attach_parametric_equalizer(&library, plan)?;
        if loudness {
            engine.attach_loudness(&library, LoudnessSettings::new(true, -2.0)?)?;
        }
        engine.load(
            LoadRequest::new(MediaInput::File(media.clone())),
            &Cancellation::default(),
        )?;
        engine.pause(false)?;
        let deadline = Instant::now() + Duration::from_secs(4);
        let (before, peq, loud) = loop {
            let state = engine.poll()?;
            let peq = engine.parametric_equalizer_stats()?;
            let loud = loudness.then(|| engine.loudness_stats()).transpose()?;
            if let Some(position) = state.position.filter(|position| *position > 0.15)
                && running(peq)
                && loud.is_none_or(running)
            {
                break (position, peq, loud);
            }
            assert!(
                Instant::now() < deadline,
                "original filters did not process PCM"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(engine.equalizer_reserved);
        assert!(engine.observe_equalizer()?.is_none());
        let geq = engine.equalizer_stats()?;
        assert!(running(geq));
        assert!(engine.apply_equalizer(EqualizerSettings::new(true, [2.0; 10])?)?);
        assert!(!engine.equalizer_reserved);
        let chain = engine.property(c"af").unwrap();
        let geq_index = chain.find("@cumulus-geq:").unwrap();
        assert!(geq_index < chain.find("@cumulus-peq:").unwrap());
        if loudness {
            assert!(chain.find("@cumulus-loudness:").unwrap() < geq_index);
        }
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            let state = engine.poll()?;
            let peq_now = engine.parametric_equalizer_stats()?;
            let loud_now = loudness.then(|| engine.loudness_stats()).transpose()?;
            assert_eq!(peq_now.instances_created, peq.instances_created);
            let geq_now = engine.equalizer_stats()?;
            assert_eq!(geq_now.instances_created, geq.instances_created);
            if let Some((after, before)) = loud_now.zip(loud) {
                assert_eq!(after.instances_created, before.instances_created);
            }
            if state
                .position
                .is_some_and(|position| position > before + 0.1)
                && running(geq_now)
                && geq_now.revision > geq.revision
                && running(peq_now)
                && peq_now.processed_samples > peq.processed_samples
                && loud_now.zip(loud).is_none_or(|(after, before)| {
                    running(after) && after.processed_samples > before.processed_samples
                })
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "ordered filters did not process PCM"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        engine.detach_parametric_equalizer()?;
        assert!(engine.parametric.is_none());
        assert!(
            engine.equalizer.is_some(),
            "activated GEQ remains independently owned"
        );
        engine.stop()?;
        engine.clear_equalizer()?;
        assert!(engine.equalizer.is_none());
        engine.attach_parametric_equalizer(&library, plan)?;
        assert!(engine.equalizer_reserved);
        engine.detach_parametric_equalizer()?;
        assert!(engine.equalizer.is_none() && engine.parametric.is_none());
        assert!(!engine.equalizer_reserved);
        engine.equalizer_library = Some(root.path().join("missing-filter.so"));
        assert!(engine.attach_parametric_equalizer(&library, plan).is_err());
        assert!(engine.equalizer.is_none() && engine.parametric.is_none());
        let chain = engine.property(c"af").unwrap_or_default();
        assert!(!chain.contains("@cumulus-peq") && !chain.contains("@cumulus-geq"));
    }
    Ok(())
}

#[test]
#[ignore = "requires explicit trusted filter library and libmpv with LADSPA"]
fn native_late_loudness_does_not_rebuild_equalizers() -> Result<()> {
    let library = PathBuf::from(std::env::var_os("CUMULUS_PROOF_EFFECT_FILTER").ok_or(
        Error::InvalidInput("supply trusted filter library via CUMULUS_PROOF_EFFECT_FILTER"),
    )?);
    let root = tempfile::tempdir()?;
    let media = root.path().join("stereo.wav");
    wav(&media)?;
    let plan = ParametricPlan::from_material(&json!({
        "aev2":{"peq":{"on":true,"gain":-2,"f":[{"band":1,"type":6,"freq":440,"gain":-4,"q":0.7}]}}
    }))?
    .unwrap();
    for with_peq in [false, true] {
        let mut engine = MpvEngine::open(MpvConfig {
            null_output: true,
            ..Default::default()
        })?
        .with_effects_library(library.clone());
        if with_peq {
            engine.attach_parametric_equalizer(&library, plan)?;
        }
        engine.apply_equalizer(EqualizerSettings::new(true, [2.0; 10])?)?;
        assert!(engine.loudness_reserved);
        assert!(engine.observe_loudness()?.is_none());
        let chain = engine.property(c"af").unwrap();
        let geq_index = chain.find("@cumulus-geq:").unwrap();
        assert!(chain.find("@cumulus-loudness:").unwrap() < geq_index);
        if with_peq {
            assert!(geq_index < chain.find("@cumulus-peq:").unwrap());
        }
        engine.load(
            LoadRequest::new(MediaInput::File(media.clone())),
            &Cancellation::default(),
        )?;
        engine.pause(false)?;
        let mut current = wait_for_effect_clock(&mut engine, with_peq, 0.15, None)?;
        let initial = current;
        for (enabled, gain) in [(true, -4.0), (false, -4.0), (true, -8.0)] {
            engine.apply_loudness(LoudnessSettings::new(enabled, gain)?)?;
            assert!(!engine.loudness_reserved);
            assert_eq!(engine.property(c"af").as_deref(), Some(chain.as_str()));
            let next = wait_for_effect_clock(
                &mut engine,
                with_peq,
                current.0 + 0.12,
                Some(current.3.revision),
            )?;
            eprintln!(
                "GEQ+PEQ={with_peq} normalization={enabled} gain={gain}: GEQ {}→{}, PEQ {:?}→{:?}, loudness {}→{}, clock {:.3}→{:.3}",
                current.1.instances_created,
                next.1.instances_created,
                current.2.map(|v| v.instances_created),
                next.2.map(|v| v.instances_created),
                current.3.instances_created,
                next.3.instances_created,
                current.0,
                next.0
            );
            for (before, after) in [(current.1, next.1), (current.3, next.3)]
                .into_iter()
                .chain(current.2.zip(next.2))
            {
                assert_eq!(after.instances_created, before.instances_created);
                assert!(after.processed_samples > before.processed_samples);
            }
            assert!(next.3.revision > current.3.revision);
            current = next;
        }
        assert!(current.0 > initial.0 + 0.36);
        engine.stop()?;
        engine.reset_track_loudness()?;
        assert!(engine.loudness_reserved);
        assert!(engine.observe_loudness()?.is_none());
        let reset_chain = engine.property(c"af").unwrap();
        assert_ne!(reset_chain, chain, "old track control ID survived reset");
        assert_eq!(
            reset_chain.split("@cumulus-geq:").nth(1),
            chain.split("@cumulus-geq:").nth(1)
        );
        assert_eq!(engine.loudness_stats()?.processed_samples, 0);
        engine.load(
            LoadRequest::new(MediaInput::File(media.clone())),
            &Cancellation::default(),
        )?;
        engine.pause(false)?;
        let new_track = wait_for_effect_clock(&mut engine, with_peq, 0.15, None)?;
        assert_eq!(new_track.3.instances_created, 2);
        assert!(engine.observe_loudness()?.is_none());
        engine.stop()?;
        // True removal still exists for the explicit stop/fault path.
        engine.clear_loudness()?;
        assert!(
            !engine
                .property(c"af")
                .unwrap()
                .contains("@cumulus-loudness")
        );
        engine.reset_track_loudness()?;
        assert!(engine.loudness_reserved);
        assert!(
            engine
                .property(c"af")
                .unwrap()
                .find("@cumulus-loudness:")
                .unwrap()
                < engine
                    .property(c"af")
                    .unwrap()
                    .find("@cumulus-geq:")
                    .unwrap()
        );
        if with_peq {
            engine.detach_parametric_equalizer()?;
            assert!(engine.loudness_reserved);
        }
        engine.detach_equalizer()?;
        assert!(!engine.loudness_reserved && engine.loudness.is_none());
        assert!(
            !engine
                .property(c"af")
                .unwrap()
                .contains("@cumulus-loudness")
        );
    }
    Ok(())
}

fn wait_for_effect_clock(
    engine: &mut MpvEngine,
    with_peq: bool,
    after: f64,
    loudness_revision: Option<u64>,
) -> Result<(f64, EqualizerStats, Option<EqualizerStats>, EqualizerStats)> {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let position = engine.poll()?.position.unwrap_or(0.0);
        let geq = engine.equalizer_stats()?;
        let peq = with_peq
            .then(|| engine.parametric_equalizer_stats())
            .transpose()?;
        let loudness = engine.loudness_stats()?;
        if position > after
            && running(geq)
            && peq.is_none_or(running)
            && running(loudness)
            && loudness_revision.is_none_or(|revision| loudness.revision > revision)
        {
            return Ok((position, geq, peq, loudness));
        }
        assert!(
            Instant::now() < deadline,
            "ordered nodes did not process PCM"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires trusted LADSPA and libmpv with FFmpeg astats/ametadata"]
fn native_late_loudness_keeps_the_combined_pcm_transition() -> Result<()> {
    let library = PathBuf::from(std::env::var_os("CUMULUS_PROOF_EFFECT_FILTER").ok_or(
        Error::InvalidInput("supply trusted filter library via CUMULUS_PROOF_EFFECT_FILTER"),
    )?);
    let root = tempfile::tempdir_in("/tmp")?;
    let media = root.path().join("stereo.wav");
    let captured = root.path().join("rms.txt");
    wav(&media)?;
    let mut engine = MpvEngine::open(MpvConfig {
        null_output: true,
        ..Default::default()
    })?
    .with_effects_library(library.clone());
    engine.control(&["set", "audio-buffer", "0.12"])?;
    let peq = ParametricPlan::from_material(&json!({
        "aev2":{"peq":{"on":true,"gain":-2,"f":[{"band":1,"type":6,"freq":440,"gain":-4,"q":0.7}]}}
    }))?
    .unwrap();
    engine.attach_parametric_equalizer(&library, peq)?;
    engine.apply_equalizer(EqualizerSettings::new(true, [2.0; 10])?)?;
    // Observe actual post-filter PCM frames without using the untimed file AO.
    // The generated /tmp path cannot contain filter graph delimiters.
    let graph = format!(
        "astats=metadata=1:reset=1:measure_perchannel=none:measure_overall=RMS_level,ametadata=mode=print:key=lavfi.astats.Overall.RMS_level:file={}:direct=1",
        captured.display()
    );
    engine.control(&["af", "add", &effect_filter("test-astats", graph)])?;
    engine.load(
        LoadRequest::new(MediaInput::File(media)),
        &Cancellation::default(),
    )?;
    engine.pause(false)?;
    let before = wait_for_effect_clock(&mut engine, true, 0.75, None)?;
    engine.apply_loudness(LoudnessSettings::new(true, -18.0)?)?;
    let attenuated = wait_for_effect_clock(&mut engine, true, 1.90, Some(before.3.revision))?;
    engine.apply_loudness(LoudnessSettings::default())?;
    let recovered = wait_for_effect_clock(&mut engine, true, 3.20, Some(attenuated.3.revision))?;
    assert_eq!(
        [
            before.1.instances_created,
            before.2.unwrap().instances_created,
            before.3.instances_created
        ],
        [
            recovered.1.instances_created,
            recovered.2.unwrap().instances_created,
            recovered.3.instances_created
        ]
    );
    engine.stop()?;
    drop(engine);
    let stats = std::fs::read_to_string(captured)?;
    let mut time = None;
    let mut frames = Vec::new();
    for line in stats.lines() {
        if let Some((_, value)) = line.split_once("pts_time:") {
            time = value.trim().parse::<f64>().ok();
        }
        if let Some(value) = line.strip_prefix("lavfi.astats.Overall.RMS_level=")
            && let (Some(time), Ok(db)) = (time.take(), value.parse::<f64>())
        {
            frames.push((time, db));
        }
    }
    let rms_db = |start, end| {
        let window: Vec<_> = frames
            .iter()
            .filter_map(|&(t, db)| (t >= start && t < end && db.is_finite()).then_some(db))
            .collect();
        assert!(
            window.len() >= 4,
            "missing native post-filter RMS window {start}..{end}: {frames:?}"
        );
        window.iter().sum::<f64>() / window.len() as f64
    };
    let dry = rms_db(0.20, 0.50);
    let wet = rms_db(1.30, 1.60);
    let restored = rms_db(2.65, 2.95);
    eprintln!(
        "post-filter PCM frames={}; dry={dry:.3} dB, wet-dry={:.3} dB, restored-dry={:.3} dB",
        frames.len(),
        wet - dry,
        restored - dry
    );
    assert!((wet - dry + 12.0).abs() < 0.4);
    assert!((restored - dry).abs() < 0.4);
    Ok(())
}
