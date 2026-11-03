//! Local component probe; no Python, DLL or account needed at runtime.
#[path = "support/effects_rotation_material.rs"]
mod material;
#[cfg(target_os = "linux")]
#[path = "support/effects_soxr_linux.rs"]
mod soxr;
#[path = "support/effects_spatial_wire.rs"]
mod wire;
use cumulus_core::effects::ncae;
use std::io::{Read, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let mode = args
        .next()
        .ok_or("usage: effect_spatial_render kernel|stage RESPONSES; internal|sidechain; material-stage RESPONSES MATERIAL.ncae | material-resampled RESPONSES MATERIAL.ncae RATE TRUSTED_SOXR (stereo f32le stdin/stdout)")?;
    let mode = mode.to_str().ok_or("invalid mode")?;
    let response_path = args.next();
    let material_path = if matches!(mode, "material-stage" | "material-resampled") {
        Some(args.next().ok_or("material-stage requires an NCAE path")?)
    } else {
        None
    };
    let resampled = if mode == "material-resampled" {
        let rate = args
            .next()
            .and_then(|value| value.into_string().ok())
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|rate| matches!(rate, 48_000 | 96_000))
            .ok_or("material-resampled requires 48000 or 96000 Hz")?;
        let library = args
            .next()
            .ok_or("material-resampled requires an absolute trusted SoXR library path")?;
        Some((rate, std::path::PathBuf::from(library)))
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
        let material = serde_json::from_slice(&decoded.plain)?;
        let table = responses.as_deref().ok_or("missing response table")?;
        match resampled {
            Some((rate, library)) => render_resampled(&material, table, &bytes, rate, &library)?,
            None => material::render(&material, table, &bytes)?,
        }
    } else {
        wire::render(mode, &bytes, responses.as_deref())?
    };
    std::io::stdout().lock().write_all(&output)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn render_resampled(
    material: &serde_json::Value,
    table: &[u8],
    input: &[u8],
    rate: u32,
    library: &std::path::Path,
) -> cumulus_core::Result<Vec<u8>> {
    // SAFETY: this explicit native CLI argument names a library the caller trusts to execute.
    let host = std::sync::Arc::new(unsafe { soxr::Host::open(library) }?);
    material::render_resampled(material, table, input, rate, host)
}

#[cfg(not(target_os = "linux"))]
fn render_resampled(
    _: &serde_json::Value,
    _: &[u8],
    _: &[u8],
    _: u32,
    _: &std::path::Path,
) -> cumulus_core::Result<Vec<u8>> {
    Err(cumulus_core::Error::State(
        "local SoXR rotation renderer requires Linux",
    ))
}
