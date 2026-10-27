//! Real libmpv verification against a private synthetic WAV, no network/account.
//! Default output is null (real decoder, discarded PCM); --auto uses the OS
//! device at zero volume. No claim of audible hardware output from null tests.
use cumulus_core::{Result, playback::engine::*};
use serde_json::json;
use std::{
    io::Write,
    time::{Duration, Instant},
};

fn wav(path: &std::path::Path) -> Result<()> {
    let rate = 48000_u32;
    let samples = rate * 4;
    let bytes = samples * 2;
    let mut file = std::fs::File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + bytes).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&rate.to_le_bytes())?;
    file.write_all(&(rate * 2).to_le_bytes())?;
    file.write_all(&2_u16.to_le_bytes())?;
    file.write_all(&16_u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&bytes.to_le_bytes())?;
    for n in 0..samples {
        let sample =
            ((n as f64 * 440.0 * std::f64::consts::TAU / rate as f64).sin() * 8000.0) as i16;
        file.write_all(&sample.to_le_bytes())?;
    }
    Ok(())
}
fn wait(
    engine: &mut dyn AudioEngine,
    condition: impl Fn(&EngineSnapshot) -> bool,
) -> Result<EngineSnapshot> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last = None;
    while Instant::now() < deadline {
        let view = engine.poll()?;
        if condition(&view) {
            return Ok(view);
        }
        if !view.events.is_empty() {
            eprintln!("engine proof event: {:?}", view.events);
        }
        last = Some(view);
        std::thread::sleep(Duration::from_millis(20));
    }
    eprintln!("engine proof timeout: {last:?}");
    Err(cumulus_core::Error::State("engine proof timed out"))
}
fn run() -> Result<serde_json::Value> {
    let auto = std::env::args().nth(1).as_deref() == Some("--auto");
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("sine.wav");
    wav(&path)?;
    let mut engine = MpvEngine::open(MpvConfig {
        null_output: !auto,
        ..Default::default()
    })?;
    let version = engine.api_version();
    engine.volume(0.0)?;
    let loaded = engine.load(
        LoadRequest::new(MediaInput::File(path.clone())),
        &Cancellation::default(),
    )?;
    assert!(loaded.duration.is_some_and(|d| (d - 4.0).abs() < 0.01));
    assert!(engine.poll()?.paused);
    engine.pause(false)?;
    let playing = wait(&mut engine, |s| s.position.is_some_and(|n| n > 0.15))?;
    engine.pause(true)?;
    let paused = engine.poll()?.position.unwrap();
    std::thread::sleep(Duration::from_millis(120));
    assert!((engine.poll()?.position.unwrap() - paused).abs() < 0.05);
    engine.seek(1.5)?;
    wait(&mut engine, |s| {
        s.position.is_some_and(|n| (n - 1.5).abs() < 0.03)
    })?;
    engine.speed(1.5)?;
    assert_eq!(engine.poll()?.speed, Some(1.5));
    assert!(engine.volume(101.0).is_err());
    assert!(engine.seek(f64::NAN).is_err());
    let mut cue = LoadRequest::new(MediaInput::File(path.clone()));
    cue.start = 2.0;
    cue.end = Some(2.35);
    let clip = engine.load(cue, &Cancellation::default())?;
    assert_ne!(clip.entry_id, loaded.entry_id);
    engine.pause(false)?;
    let eof = wait(&mut engine, |s| {
        s.events.iter().any(|e|matches!(e,EngineEvent::Ended{entry_id,reason:EndReason::Eof,..} if *entry_id==clip.entry_id))
    })?;
    let restored = engine.load(
        LoadRequest::new(MediaInput::File(path.clone())),
        &Cancellation::default(),
    )?;
    engine.seek(3.0)?;
    engine.pause(false)?;
    wait(&mut engine, |s| s.position.is_some_and(|n| n > 3.1))?;
    engine.stop()?;
    assert!(engine.poll()?.entry_id.is_none());
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(matches!(
        engine.load(LoadRequest::new(MediaInput::File(path)), &cancel),
        Err(cumulus_core::Error::Cancelled)
    ));
    Ok(
        json!({"ok":true,"libmpv_api":version,"decoder":loaded.codec,"duration":loaded.duration,"output":playing.audio_output,"requested_output":if auto{"auto_muted"}else{"null"},"progress":true,"pause":true,"seek":true,"speed":true,"bounds_validation":true,"cue_engine_eof":!eof.events.is_empty(),"cue_end_reset":restored.entry_id!=clip.entry_id,"cancel":true,"remote_requests":0}),
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
