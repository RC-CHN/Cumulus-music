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
