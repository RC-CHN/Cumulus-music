//! Offline probe of actual Rust wrappers. No DLL, Python or account at runtime.
#[path = "support/limiter_wire.rs"]
mod limiter_wire;
#[path = "support/stereo_zero_wire.rs"]
mod stereo_zero_wire;
#[path = "support/effects_dsp_wire.rs"]
mod wire;
use cumulus_core::effects::{material, ncae};
use std::io::{Read, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::args().nth(1).ok_or(
        "supply coeff, peq-coeff, biquad, bt, geq-outer, peq, stereo-zero or material-{route,eq,bt,peq,rotate,stereo,cmp,limiter,limiter-plan,limiter-flow,dly,alc,spspatial}; for NCAE PCM use material-render-active-peq PATH or material-limiter-{render,stream} PATH",
    )?;
    let mut input = Vec::new();
    std::io::stdin()
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut input)?;
    if input.len() > 16 * 1024 * 1024 {
        return Err("input too large".into());
    }
    if matches!(
        mode.as_str(),
        "material-limiter-render" | "material-limiter-stream"
    ) {
        let path = std::env::args_os()
            .nth(2)
            .ok_or("limiter PCM modes require an NCAE path")?;
        if std::env::args_os().nth(3).is_some() {
            return Err("limiter PCM modes do not accept a component override".into());
        }
        let output = if mode == "material-limiter-stream" {
            limiter_wire::stream_ncae(path, &input)?
        } else {
            limiter_wire::render_ncae(path, &input)?
        };
        println!("{}", serde_json::to_string(&output)?);
        return Ok(());
    }
    if mode == "stereo-zero" {
        println!(
            "{}",
            serde_json::to_string(&stereo_zero_wire::render(&input)?)?
        );
        return Ok(());
    }
    let result = if mode == "material-render-active-peq" {
        let path = std::env::args_os()
            .nth(2)
            .ok_or("material-render-active-peq requires an NCAE path")?;
        if std::env::args_os().nth(3).is_some() {
            return Err("material-render-active-peq does not accept a section override".into());
        }
        wire::render_routed_ncae_peq(path, &input)?
    } else if mode == "material-render" {
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
            "material-limiter-plan" => {
                serde_json::json!({"limiter": material::material_limiter_plan(&value)?, "dspRendered": false})
            }
            "material-limiter-flow" => describe_limiter_flow(value)?,
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

fn describe_limiter_flow(
    input: serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Request {
        material: serde_json::Value,
        sample_rate: u32,
        channels: u32,
        max_frames: u32,
        blocks: Vec<Block>,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Block {
        frames: u32,
        end: bool,
    }
    let input: Request = serde_json::from_value(input)?;
    if input.blocks.len() > 16_384 {
        return Err("too many limiter blocks".into());
    }
    let plan = material::material_limiter_plan(&input.material)?;
    let mut flow = plan.frame_flow(input.sample_rate, input.channels, input.max_frames)?;
    let blocks = input
        .blocks
        .into_iter()
        .map(|block| flow.next_block(block.frames, block.end))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(
        serde_json::json!({"lookaheadFrames":flow.lookahead_frames(), "blocks":blocks, "dspRendered":false}),
    )
}
