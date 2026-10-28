//! Interactive native-device proof, driven by scripts/ci/output_devices_smoke.py.
//! Uses only generated silence and an explicitly isolated PulseAudio server.
//! The driver independently observes sink inputs; configured != actual endpoint.
use cumulus_core::{Error, Result, playback::engine::*};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    path::Path,
    time::Duration,
};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Devices,
    Select { id: String },
    Load,
    Pause { paused: bool },
    State,
    Quit,
}
fn silence(path: &Path) -> Result<()> {
    let rate = 48000u32;
    let bytes = rate * 2 * 60;
    let mut file = std::fs::File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + bytes).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&rate.to_le_bytes())?;
    file.write_all(&(rate * 2).to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&bytes.to_le_bytes())?;
    for _ in 0..60 {
        file.write_all(&vec![0; rate as usize * 2])?;
    }
    Ok(())
}
fn reply(value: Value) -> Result<()> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{value}")?;
    out.flush()?;
    Ok(())
}
pub fn run() -> Result<Value> {
    let server = std::env::var("PULSE_SERVER").unwrap_or_default();
    let path = server
        .strip_prefix("unix:")
        .map(Path::new)
        .ok_or(Error::State("isolated pulse proof server required"))?;
    let root = path
        .parent()
        .and_then(Path::parent)
        .ok_or(Error::State("isolated pulse proof server required"))?;
    if !root
        .file_name()
        .is_some_and(|s| s.to_string_lossy().starts_with("cumulus-output-proof-"))
        || !path.exists()
    {
        return Err(Error::State("isolated pulse proof server required"));
    }
    let dir = tempfile::tempdir()?;
    let wave = dir.path().join("silence.wav");
    silence(&wave)?;
    let mut engine = MpvEngine::open(MpvConfig::default())?;
    engine.volume(0.0)?;
    reply(json!({"ready":true}))?;
    let mut selected = String::new();
    let mut loaded = false;
    for (commands, line) in std::io::stdin().lock().lines().enumerate() {
        let command: Command = serde_json::from_str(&line?)?;
        if matches!(command, Command::Quit) {
            engine.stop()?;
            reply(json!({"ok":true}))?;
            drop(engine);
            return Ok(
                json!({"ok":true,"commands":commands,"remote_requests":0,"synthetic_silence":true}),
            );
        }
        let result = (|| -> Result<Value> {
            match command {
                Command::Devices => Ok(serde_json::to_value(engine.output_devices()?)?),
                Command::Select { id } => {
                    // Even an accidental driver command must never enable a
                    // host backend. Invalid IDs below still exercise the real
                    // native validator; only our private pulse sinks can load.
                    let invalid = id.is_empty() || id.contains('\0') || id.len() > 4096;
                    if !(invalid
                        || id.starts_with("pulse/cumulus-proof-")
                        || (id == "auto" && !loaded))
                    {
                        return Err(Error::State("device is outside the isolated proof"));
                    }
                    engine.set_output_device(&id)?;
                    selected = id;
                    Ok(Value::Null)
                }
                Command::Load => {
                    // Never decode using auto or a host hardware backend.
                    if !matches!(
                        selected.as_str(),
                        "pulse/cumulus-proof-a" | "pulse/cumulus-proof-b"
                    ) {
                        return Err(Error::State("select an isolated proof sink first"));
                    }
                    let mut request = LoadRequest::new(MediaInput::File(wave.clone()));
                    request.timeout = Duration::from_secs(5);
                    let entry = engine.load(request, &Cancellation::default())?.entry_id;
                    loaded = true;
                    Ok(json!({"entry":entry}))
                }
                Command::Pause { paused } => {
                    engine.pause(paused)?;
                    Ok(Value::Null)
                }
                Command::State => {
                    let s = engine.poll()?;
                    Ok(
                        json!({"entry":s.entry_id,"paused":s.paused,"position":s.position,
                        "volume":s.volume,"driver":s.audio_output,"events":s.events.len()}),
                    )
                }
                Command::Quit => unreachable!(),
            }
        })();
        reply(match result {
            Ok(value) => json!({"ok":true,"value":value}),
            Err(error) => json!({"ok":false,"error":error.to_string()}),
        })?;
    }
    Err(Error::State("device proof ended without quit"))
}
fn main() {
    if std::env::args().nth(1).as_deref() != Some("--isolated-pulse") {
        eprintln!("use the isolated output_devices_smoke.py driver");
        std::process::exit(2);
    }
    match run() {
        Ok(report) => println!("{report}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
