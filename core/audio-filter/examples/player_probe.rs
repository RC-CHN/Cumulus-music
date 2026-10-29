//! Real decoder -> FFmpeg LADSPA -> recovered C DSP -> PCM file, then live
//! controls through the production MpvEngine. Synthetic media only; no network.
#[path = "support/mpv_capture.rs"]
mod mpv_capture;
#[path = "support/synthetic_wave.rs"]
mod synthetic_wave;
use cumulus_core::{
    Error, Result,
    effects::{
        dsp::{Format, GraphicEq, PlanarBlock},
        equalizer::{EqualizerHost, EqualizerSettings, abi::CAPACITY},
    },
    playback::engine::*,
};
use serde_json::json;
use std::{
    io::{Seek, SeekFrom, Write},
    path::Path,
    time::{Duration, Instant},
};
use synthetic_wave::wav;

fn render(input: &[f32], rate: u32, channels: usize, gains: [f32; 10]) -> Result<Vec<f32>> {
    let format = Format::new(rate as f32, channels as u32, CAPACITY)?;
    let mut stage = GraphicEq::new(format)?;
    stage.set_gains(&gains)?;
    let mut block = PlanarBlock::new(format);
    let mut out = Vec::with_capacity(input.len());
    for part in input.chunks(CAPACITY as usize * channels) {
        let frames = block.load_interleaved(part)?;
        stage.process(&mut block, frames)?;
        let mut interleaved = vec![0.0; part.len()];
        block.copy_interleaved(frames, &mut interleaved)?;
        out.extend(interleaved);
    }
    Ok(out)
}
fn wait(engine: &mut MpvEngine, check: impl Fn(&EngineSnapshot) -> bool) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
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
            return Err(Error::State("player failed during EQ proof"));
        }
        if check(&state) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err(Error::State("equalizer playback proof timed out"))
}
fn run() -> Result<serde_json::Value> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "player_probe FILTER_LIBRARY NEW_OUTPUT_DIR",
        ));
    }
    let library = Path::new(&args[0]).canonicalize()?;
    let dir = Path::new(&args[1]);
    std::fs::create_dir(dir)?;
    let dir = dir.canonicalize()?;
    // Exercise both lavfi quoting layers, UTF-8 and mpv's byte-length syntax.
    let odd = dir.join(if cfg!(target_os = "windows") {
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
    std::fs::copy(&library, &library_copy)?;
    let gains = [4.0, -3.0, 1.0, 0.0, -2.0, 3.0, 6.0, 0.0, 2.0, -4.0];
    let settings = EqualizerSettings::new(true, gains)?;
    let mut cases = Vec::new();
    for (rate, channels) in [(44100, 1), (48000, 2), (96000, 6)] {
        let name = format!("{rate}-{channels}");
        let file = dir.join(format!("{name}.wav"));
        let output = dir.join(format!("{name}.pcm"));
        let input = wav(&file, rate, channels, rate as usize + 13)?;
        let host = EqualizerHost::open(&library_copy, settings)?;
        mpv_capture::capture(&file, &output, &host.filter_graph(), rate, channels)?;
        let bytes = std::fs::read(&output)?;
        let actual: Vec<_> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let expected = render(&input, rate, channels, gains)?;
        std::fs::write(
            dir.join(format!("{name}-expected.pcm")),
            expected
                .iter()
                .flat_map(|x| x.to_le_bytes())
                .collect::<Vec<_>>(),
        )?;
        if bytes.len() != expected.len() * 4 {
            return Err(Error::State("captured PCM length mismatch"));
        }
        let error = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        if actual.iter().any(|x| !x.is_finite()) || error > 2e-6 {
            eprintln!(
                "PCM case {name}: max_abs_error={error}, samples={}",
                actual.len()
            );
            return Err(Error::State("captured PCM differs from recovered DSP"));
        }
        let changed = actual
            .iter()
            .zip(&input)
            .any(|(a, b)| (a - b).abs() > 0.001);
        if !changed {
            return Err(Error::State("captured PCM did not apply EQ"));
        }
        let stats = host.stats()?;
        if stats.processed_samples != input.len() as u64
            || stats.failed_blocks != 0
            || stats.active_instances != 0
        {
            return Err(Error::State("adapter processing counters mismatch"));
        }
        cases.push(json!({"rate":rate,"channels":channels,"samples":input.len(),"max_abs_error":error,"changed":changed,"stats":stats}));
    }
    let source = dir.join("controls.wav");
    wav(&source, 48000, 2, 48000 * 5)?;
    let mut engine = MpvEngine::open(MpvConfig {
        null_output: true,
        ..Default::default()
    })?;
    engine.attach_equalizer(&library_copy, settings)?;
    assert_eq!(engine.equalizer_stats()?.processed_samples, 0);
    engine.load(
        LoadRequest::new(MediaInput::File(source.clone())),
        &Cancellation::default(),
    )?;
    engine.pause(false)?;
    wait(&mut engine, |s| s.position.is_some_and(|p| p > 0.12))?;
    let first = engine.equalizer_stats()?;
    engine.set_equalizer(EqualizerSettings::new(false, gains)?)?;
    wait(&mut engine, |s| s.position.is_some_and(|p| p > 0.5))?;
    let off = engine.equalizer_stats()?;
    assert!(off.observed_revision > first.observed_revision);
    engine.pause(true)?;
    engine.seek(1.0)?;
    engine.set_equalizer(EqualizerSettings::new(true, [-2.0; 10])?)?;
    engine.pause(false)?;
    wait(&mut engine, |s| s.position.is_some_and(|p| p > 1.25))?;
    let sought = engine.equalizer_stats()?;
    assert_eq!(sought.revision, sought.observed_revision);
    engine.load(
        LoadRequest::new(MediaInput::File(source.clone())),
        &Cancellation::default(),
    )?;
    engine.pause(false)?;
    wait(&mut engine, |s| s.position.is_some_and(|p| p > 0.2))?;
    let reload = engine.equalizer_stats()?;
    assert_eq!(reload.revision, sought.revision);
    assert!(reload.instances_created > sought.instances_created);
    assert_eq!(reload.failed_blocks, 0);
    engine.detach_equalizer()?;
    assert!(engine.equalizer_stats().is_err());
    engine.stop()?;
    let fault_file = dir.join("nonfinite.wav");
    wav(&fault_file, 48000, 2, 48000)?;
    let mut patch = std::fs::OpenOptions::new().write(true).open(&fault_file)?;
    patch.seek(SeekFrom::Start(44 + 4096 * 4))?;
    patch.write_all(&f32::NAN.to_le_bytes())?;
    drop(patch);
    let fault_output = dir.join("nonfinite.pcm");
    let fault_host = EqualizerHost::open(&library_copy, settings)?;
    mpv_capture::capture(
        &fault_file,
        &fault_output,
        &fault_host.filter_graph(),
        48000,
        2,
    )?;
    let fault_capture = fault_host.stats()?;
    let captured = std::fs::read(&fault_output)?;
    assert_eq!(captured.len(), 48000 * 2 * 4);
    assert!(
        captured
            .chunks_exact(4)
            .all(|b| f32::from_le_bytes(b.try_into().unwrap()).is_finite())
    );
    engine.attach_equalizer(&library_copy, settings)?;
    engine.load(
        LoadRequest::new(MediaInput::File(fault_file)),
        &Cancellation::default(),
    )?;
    engine.pause(false)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut surfaced = false;
    while Instant::now() < deadline {
        match engine.poll() {
            Err(Error::Media("equalizer processing failed; recreate the adapter")) => {
                surfaced = true;
                break;
            }
            Err(error) => return Err(error),
            Ok(state) if state.entry_id.is_none() => break,
            Ok(_) => {}
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let fault = engine.equalizer_stats()?;
    if fault.failed_blocks != 0 {
        assert!(surfaced);
    } else {
        // Some mpv format-conversion paths normalize NaN before our callback.
        // Count full processing, but do not report the engine error branch as
        // exercised. The direct callback test separately proves quarantine.
        assert!(!surfaced);
        assert_eq!(fault.processed_samples, 96000);
    }
    engine.stop()?;
    engine.detach_equalizer()?;
    engine.attach_equalizer(&library_copy, settings)?;
    engine.load(
        LoadRequest::new(MediaInput::File(source)),
        &Cancellation::default(),
    )?;
    engine.pause(false)?;
    wait(&mut engine, |s| s.position.is_some_and(|p| p > 0.2))?;
    let recovered = engine.equalizer_stats()?;
    assert_eq!(recovered.failed_blocks, 0);
    engine.stop()?;
    engine.detach_equalizer()?;
    Ok(
        json!({"ok":true,"remote_requests":0,"pcm_cases":cases,"controls":{"first":first,"disabled":off,"seek":sought,"reload":reload,"detach":true},"nonfinite_input":{"engine_error_exercised":surfaced,"stats":fault,"pcm_capture_stats":fault_capture,"pcm_output_finite":true,"reopened":recovered},"actual_device_listening":false,"ui_integrated":false}),
    )
}
fn main() {
    match run() {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1)
        }
    }
}
