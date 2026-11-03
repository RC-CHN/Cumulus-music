//! Offline probe of actual Rust wrappers. No DLL, Python or account at runtime.
#[path = "support/effects_dsp_wire.rs"]
mod wire;
use cumulus_core::effects::{material, ncae};
use std::io::{Read, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::args().nth(1).ok_or(
        "supply coeff, peq-coeff, biquad, bt, geq-outer, peq or material-{route,eq,bt,peq,rotate,stereo,cmp,limiter,dly,alc,spspatial}",
    )?;
    let mut input = Vec::new();
    std::io::stdin()
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut input)?;
    if input.len() > 16 * 1024 * 1024 {
        return Err("input too large".into());
    }
    let result = if mode == "material-render" {
        let path = std::env::args_os()
            .nth(2)
            .ok_or("material-render requires an NCAE path")?;
        let section = std::env::args()
            .nth(3)
            .ok_or("material-render requires an explicit PEQ section")?;
        let decoded = ncae::open(path, ncae::Limits::default())?;
        wire::render_peq_material(&serde_json::from_slice(&decoded.plain)?, &section, &input)?
    } else if mode.starts_with("material-") {
        let value = serde_json::from_slice(&input)?;
        let calls = match mode.as_str() {
            "material-route" => {
                let routing = material::route_sections(&value)?;
                serde_json::json!({"schema":routing.schema,"sections":routing.sections,"dspRendered":false})
            }
            "material-eq" => serde_json::to_value(material::graphic_calls(&value)?)?,
            "material-bt" => serde_json::to_value(material::bass_treble_calls(&value)?)?,
            "material-cmp" => serde_json::to_value(material::compressor_calls(&value)?)?,
            "material-limiter" => serde_json::to_value(material::limiter_calls(&value)?)?,
            "material-dly" => serde_json::to_value(material::delay_calls(&value)?)?,
            "material-alc" => serde_json::to_value(material::alc_calls(&value)?)?,
            "material-spspatial" => serde_json::to_value(material::speaker_spatial_calls(&value)?)?,
            "material-peq" => serde_json::to_value(material::peq_calls(&value)?)?,
            "material-rotate" => serde_json::to_value(material::rotation_calls(&value)?)?,
            "material-stereo" => serde_json::to_value(material::stereo_calls(&value)?)?,
            _ => return Err("unknown material probe".into()),
        };
        println!("{}", serde_json::to_string(&calls)?);
        return Ok(());
    } else {
        wire::render(&mode, &input)?
    };
    let mut output = std::io::stdout().lock();
    for value in result {
        output.write_all(&value.to_le_bytes())?;
    }
    Ok(())
}
