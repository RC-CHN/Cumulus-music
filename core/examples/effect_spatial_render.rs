//! Local component probe; no Python, DLL or account needed at runtime.
#[path = "support/effects_rotation_material.rs"]
mod material;
#[path = "support/effects_spatial_wire.rs"]
mod wire;
use cumulus_core::effects::ncae;
use std::io::{Read, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let mode = args
        .next()
        .ok_or("usage: effect_spatial_render kernel|stage RESPONSES; internal|sidechain; material-stage RESPONSES MATERIAL.ncae (stereo 44100Hz f32le stdin/stdout)")?;
    let mode = mode.to_str().ok_or("invalid mode")?;
    let response_path = args.next();
    let material_path = if mode == "material-stage" {
        Some(args.next().ok_or("material-stage requires an NCAE path")?)
    } else {
        None
    };
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let responses = if let Some(path) = response_path {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(cumulus_core::effects::dsp::RotationResponses::BYTE_LENGTH as u64 + 1)
            .read_to_end(&mut bytes)?;
        Some(bytes)
    } else {
        None
    };
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    let output = if let Some(path) = material_path {
        let decoded = ncae::open(path, ncae::Limits::default())?;
        material::render(
            &serde_json::from_slice(&decoded.plain)?,
            responses.as_deref().ok_or("missing response table")?,
            &bytes,
        )?
    } else {
        wire::render(mode, &bytes, responses.as_deref())?
    };
    std::io::stdout().lock().write_all(&output)?;
    Ok(())
}
