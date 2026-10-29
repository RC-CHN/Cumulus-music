//! Real decoder/filter PCM, then production MpvEngine hot changes and recovery.
//! Desktop native_audio_probe calls verify only after its real GUI initializes.
#[path = "support/mpv_capture.rs"]
mod mpv_capture;
#[path = "support/synthetic_wave.rs"]
mod synthetic_wave;
use cumulus_core::{
    Error, Result,
    effects::{
        dsp::{Format, GraphicEq, LoudnessParameters, LoudnessStage, PlanarBlock},
        equalizer::{EqualizerHost, EqualizerSettings},
        loudness::{LoudnessHost, LoudnessSettings, abi::CAPACITY},
    },
    playback::engine::*,
};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn render(
    input: &[f32],
    rate: u32,
    channels: usize,
    settings: LoudnessSettings,
    eq: bool,
) -> Result<Vec<f32>> {
    let format = Format::new(rate as f32, channels as u32, CAPACITY)?;
    let mut loudness = LoudnessStage::new(format);
    loudness.set_bypass(!settings.enabled())?;
    if settings.enabled() {
        loudness.set_parameters(LoudnessParameters {
            gain_db: settings.gain_db(),
            peak: 0.0,
        })?;
    }
    loudness.reset();
    let mut equalizer = GraphicEq::new(format)?;
    equalizer.set_gains(&[3.0; 10])?;
    let mut block = PlanarBlock::new(format);
    let mut output = Vec::with_capacity(input.len());
    for part in input.chunks(CAPACITY as usize * channels) {
        let frames = block.load_interleaved(part)?;
        loudness.process(&mut block, frames)?;
        if eq {
            equalizer.process(&mut block, frames)?;
        }
        let offset = output.len();
        output.resize(offset + part.len(), 0.0);
        block.copy_interleaved(frames, &mut output[offset..])?;
    }
    Ok(output)
}
fn wait(engine: &mut MpvEngine, position: f64) -> Result<()> {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        let state = engine.poll()?;
        if state.events.iter().any(|e| {
            matches!(
                e,
                EngineEvent::Ended {
                    reason: EndReason::Error,
                    ..
                }
            )
        }) {
            return Err(Error::State("loudness proof playback failed"));
        }
        if state.position.is_some_and(|p| p > position) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err(Error::State("loudness playback proof timeout"))
}
pub fn verify(library: &Path, root: &Path) -> Result<Value> {
    let library = library.canonicalize()?;
    std::fs::create_dir(root)?;
    let root = root.canonicalize()?;
    let odd = root.join(if cfg!(target_os = "windows") {
        "滤镜 [test], quote' space"
    } else {
        "滤镜 [test], quote' colon: space"
    });
    std::fs::create_dir(&odd)?;
    let library_copy = odd.join(
        library
            .file_name()
            .ok_or(Error::InvalidInput("library filename"))?,
    );
    std::fs::copy(library, &library_copy)?;
    let mut cases = Vec::new();
    for (rate, channels) in [(44100, 1), (48000, 2), (96000, 6)] {
        let source = root.join(format!("{rate}-{channels}.wav"));
        let input = synthetic_wave::wav(&source, rate, channels, rate as usize / 4 + 13)?;
        for (index, (enabled, gain, eq)) in [
            (true, -18.0, false),
            (true, 3.0, false),
            (false, -18.0, false),
            (true, -12.0, true),
        ]
        .into_iter()
        .enumerate()
        {
            let settings = LoudnessSettings::new(enabled, gain)?;
            let host = LoudnessHost::open(&library_copy, settings)?;
            let equalizer = if eq {
                Some(EqualizerHost::open(
                    &library_copy,
                    EqualizerSettings::new(true, [3.0; 10])?,
                )?)
            } else {
                None
            };
            let graph = if let Some(equalizer) = &equalizer {
                format!("{},{}", host.filter_graph(), equalizer.filter_graph())
            } else {
                host.filter_graph()
            };
            let pcm = root.join(format!("{rate}-{channels}-{index}.pcm"));
            mpv_capture::capture(&source, &pcm, &graph, rate, channels)?;
            let bytes = std::fs::read(&pcm)?;
            let actual: Vec<_> = bytes
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            let expected = render(&input, rate, channels, settings, eq)?;
            if bytes.len() != expected.len() * 4 {
                return Err(Error::State("loudness PCM length mismatch"));
            }
            let error = actual
                .iter()
                .zip(&expected)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f32, f32::max);
            if actual.iter().any(|v| !v.is_finite()) || error > 2e-6 {
                return Err(Error::State("loudness PCM differs from recovered DSP"));
            }
            let changed = actual
                .iter()
                .zip(&input)
                .any(|(a, b)| (a - b).abs() > 0.001);
            if changed != enabled {
                return Err(Error::State("loudness dry/wet PCM mismatch"));
            }
            let stats = host.stats()?;
            if stats.processed_samples != input.len() as u64
                || stats.failed_blocks != 0
                || stats.active_instances != 0
            {
                return Err(Error::State("loudness processing counters mismatch"));
            }
            cases.push(json!({"rate":rate,"channels":channels,"gain":gain,"enabled":enabled,"equalizer":eq,"samples":input.len(),"maxAbsError":error,"changed":changed,"stats":stats}));
        }
    }
    let source = root.join("controls.wav");
    synthetic_wave::wav(&source, 48000, 2, 48000 * 4)?;
    let mut engine = MpvEngine::open(MpvConfig {
        null_output: true,
        ..Default::default()
    })?
    .with_effects_library(library_copy);
    // Attach GEQ first; the loudness adapter must prepend itself independently.
    assert!(engine.apply_equalizer(EqualizerSettings::new(true, [3.0; 10])?)?);
    assert!(engine.apply_loudness(LoudnessSettings::new(true, -18.0)?)?);
    assert_eq!(engine.loudness_stats()?.processed_samples, 0);
    engine.load(
        LoadRequest::new(MediaInput::File(source.clone())),
        &Cancellation::default(),
    )?;
    engine.pause(false)?;
    wait(&mut engine, 0.12)?;
    let first = engine.loudness_stats()?;
    engine.set_loudness(LoudnessSettings::new(false, -18.0)?)?;
    wait(&mut engine, 0.5)?;
    let bypass = engine.loudness_stats()?;
    assert!(bypass.observed_revision > first.observed_revision);
    engine.pause(true)?;
    engine.seek(1.0)?;
    engine.set_loudness(LoudnessSettings::new(true, 3.0)?)?;
    engine.pause(false)?;
    wait(&mut engine, 1.3)?;
    let seek = engine.loudness_stats()?;
    assert_eq!(seek.revision, seek.observed_revision);
    engine.clear_loudness()?;
    assert!(engine.observe_loudness()?.is_none());
    assert_eq!(engine.equalizer_stats()?.failed_blocks, 0);
    assert!(engine.apply_loudness(LoudnessSettings::new(true, -12.0)?)?);
    engine.load(
        LoadRequest::new(MediaInput::File(source)),
        &Cancellation::default(),
    )?;
    engine.pause(false)?;
    wait(&mut engine, 0.2)?;
    let reloaded = engine.loudness_stats()?;
    assert!(reloaded.processed_samples > 0);
    assert_eq!(reloaded.failed_blocks, 0);
    engine.clear_equalizer()?;
    engine.clear_loudness()?;
    engine.stop()?;
    let report = json!({"ok":true,"cases":cases,"controls":{"first":first,"bypass":bypass,"seek":seek,"reloaded":reloaded},"actualDecoder":true,"physicalAudioOutputVerified":false,"businessPreferenceTested":false,"realMusicRequests":0});
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "loudness_probe FILTER_LIBRARY NEW_OUTPUT_DIR",
        ));
    }
    println!("{}", verify(Path::new(&args[0]), Path::new(&args[1]))?);
    Ok(())
}
