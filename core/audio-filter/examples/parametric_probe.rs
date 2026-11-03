//! Actual libmpv native-only PEQ path with synthetic stereo WAV/null output.
use cumulus_core::{
    Error, Result,
    effects::parametric::ParametricPlan,
    playback::engine::{AudioEngine, Cancellation, LoadRequest, MediaInput, MpvConfig, MpvEngine},
};
use serde_json::json;
use std::{
    io::{BufWriter, Write},
    path::Path,
    time::{Duration, Instant},
};

fn wav(path: &Path, rate: u32) -> Result<()> {
    let frames = 2 * rate;
    let bytes = 4 * frames;
    let mut file = BufWriter::new(std::fs::File::create(path)?);
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + bytes).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&2_u16.to_le_bytes())?;
    file.write_all(&rate.to_le_bytes())?;
    file.write_all(&(rate * 4).to_le_bytes())?;
    file.write_all(&4_u16.to_le_bytes())?;
    file.write_all(&16_u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&bytes.to_le_bytes())?;
    for i in 0..frames {
        for frequency in [440.0, 997.0] {
            let sample = (f64::from(i) * frequency * std::f64::consts::TAU / f64::from(rate)).sin();
            file.write_all(&((sample * 4000.0) as i16).to_le_bytes())?;
        }
    }
    file.flush()?;
    Ok(())
}

fn main() -> Result<()> {
    let library = std::env::args_os().nth(1).ok_or(Error::InvalidInput(
        "supply the trusted audio-filter library path",
    ))?;
    let root = tempfile::tempdir()?;
    let file = root.path().join("stereo.wav");
    let plan = ParametricPlan::from_material(&json!({"aev2":{"peq":{
        "on":true,"gain":-2,"f":[{"band":1,"type":6,"freq":440,"gain":-4,"q":0.7}]
    }}}))?
    .ok_or(Error::State("test material did not route PEQ"))?;
    let mut stages = Vec::new();
    for rate in [44_100, 96_000] {
        wav(&file, rate)?;
        let mut engine = MpvEngine::open(MpvConfig {
            null_output: true,
            ..Default::default()
        })?
        .with_effects_library(Path::new(&library).to_path_buf());
        assert!(engine.apply_parametric_component(plan)?);
        assert_eq!(engine.parametric_equalizer_stats()?.processed_samples, 0);
        engine.load(
            LoadRequest::new(MediaInput::File(file.clone())),
            &Cancellation::default(),
        )?;
        engine.pause(false)?;
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let view = engine.poll()?;
            let stats = engine.parametric_equalizer_stats()?;
            if view.position.is_some_and(|position| position > 0.15)
                && stats.active_instances == 2
                && stats.processed_samples > 1024
            {
                assert_eq!(stats.failed_blocks, 0);
                assert_eq!(stats.observed_revision, stats.revision);
                stages.push(json!({"sourceSampleRate":rate,"stats":stats}));
                break;
            }
            if Instant::now() >= deadline {
                return Err(Error::State(
                    "native stereo PEQ proof did not process audio",
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        engine.pause(true)?;
        engine.clear_parametric_component()?;
        assert!(engine.parametric_equalizer_stats().is_err());
        assert!(engine.poll()?.paused);
        engine.stop()?;
    }
    println!(
        "{}",
        json!({"nativeLibmpv":true,"hardwareOutput":false,"cases":stages})
    );
    Ok(())
}
