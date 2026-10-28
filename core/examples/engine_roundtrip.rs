//! Real libmpv verification against a private synthetic WAV, no network/account.
//! Default output is null (real decoder, discarded PCM); --auto uses the OS
//! device at zero volume. No claim of audible hardware output from null tests.
use cumulus_core::{
    Result,
    library::probe::{AudioProbe, Ffprobe},
    playback::engine::*,
};
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
pub fn verify(auto: bool) -> Result<serde_json::Value> {
    verify_with_observer(auto, |_| {})
}
pub fn verify_with_observer(
    auto: bool,
    mut progress: impl FnMut(&'static str),
) -> Result<serde_json::Value> {
    progress("fixture_start");
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("sine.wav");
    wav(&path)?;
    progress("metadata_start");
    let metadata = Ffprobe::default().inspect(&path)?;
    assert_eq!(metadata.format.as_deref(), Some("pcm_s16le"));
    assert_eq!(metadata.sample_rate, Some(48000));
    assert_eq!(metadata.channels, Some(1));
    assert!((metadata.duration - 4.0).abs() < 0.01);
    progress("engine_start");
    let mut engine = MpvEngine::open(MpvConfig {
        null_output: !auto,
        ..Default::default()
    })?;
    let version = engine.api_version();
    progress("engine_ready");
    engine.volume(0.0)?;
    let loaded = engine.load(
        LoadRequest::new(MediaInput::File(path.clone())),
        &Cancellation::default(),
    )?;
    progress("media_loaded");
    assert!(loaded.duration.is_some_and(|d| (d - 4.0).abs() < 0.01));
    assert!(engine.poll()?.paused);
    engine.pause(false)?;
    let playing = wait(&mut engine, |s| s.position.is_some_and(|n| n > 0.15))?;
    progress("progress_verified");
    engine.pause(true)?;
    let paused = engine.poll()?.position.unwrap();
    std::thread::sleep(Duration::from_millis(120));
    assert!((engine.poll()?.position.unwrap() - paused).abs() < 0.05);
    progress("pause_verified");
    engine.seek(1.5)?;
    // libmpv's null output can report target minus its queued output latency
    // while paused, even after playback-restart. Verify pause is preserved,
    // then check the actual resumed clock against elapsed time. Waiting here
    // also prevents a transient target value from making the proof pass early.
    std::thread::sleep(Duration::from_millis(150));
    assert!(engine.poll()?.paused);
    let resumed_at = Instant::now();
    engine.pause(false)?;
    wait(&mut engine, |s| {
        s.position
            .is_some_and(|n| (n - resumed_at.elapsed().as_secs_f64() - 1.5).abs() < 0.03)
    })?;
    progress("seek_verified");
    engine.pause(true)?;
    engine.speed(1.5)?;
    assert_eq!(engine.poll()?.speed, Some(1.5));
    assert!(engine.volume(101.0).is_err());
    assert!(engine.seek(f64::NAN).is_err());
    progress("speed_bounds_verified");
    let mut cue = LoadRequest::new(MediaInput::File(path.clone()));
    cue.start = 2.0;
    cue.end = Some(2.35);
    let clip = engine.load(cue, &Cancellation::default())?;
    assert_ne!(clip.entry_id, loaded.entry_id);
    engine.pause(false)?;
    let eof = wait(&mut engine, |s| {
        s.events.iter().any(|e|matches!(e,EngineEvent::Ended{entry_id,reason:EndReason::Eof,..} if *entry_id==clip.entry_id))
    })?;
    progress("cue_eof_verified");
    let restored = engine.load(
        LoadRequest::new(MediaInput::File(path.clone())),
        &Cancellation::default(),
    )?;
    engine.seek(3.0)?;
    engine.pause(false)?;
    wait(&mut engine, |s| s.position.is_some_and(|n| n > 3.1))?;
    progress("cue_reset_verified");
    engine.stop()?;
    assert!(engine.poll()?.entry_id.is_none());
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(matches!(
        engine.load(LoadRequest::new(MediaInput::File(path)), &cancel),
        Err(cumulus_core::Error::Cancelled)
    ));
    progress("cancel_verified");
    progress("engine_closing");
    drop(engine);
    progress("engine_closed");
    Ok(
        json!({"ok":true,"metadata_probe":true,"libmpv_api":version,"decoder":loaded.codec,"duration":loaded.duration,"output":playing.audio_output,"requested_output":if auto{"auto_muted"}else{"null"},"progress":true,"pause":true,"seek":true,"paused_seek_verified_on_resume":true,"speed":true,"bounds_validation":true,"cue_engine_eof":!eof.events.is_empty(),"cue_end_reset":restored.entry_id!=clip.entry_id,"cancel":true,"remote_requests":0}),
    )
}
fn main() {
    match verify(std::env::args().nth(1).as_deref() == Some("--auto")) {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1)
        }
    }
}
