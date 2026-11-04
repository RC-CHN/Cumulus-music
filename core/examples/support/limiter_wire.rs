use cumulus_core::effects::{
    dsp::{Format, PlanarBlock},
    material::material_limiter_plan,
    ncae,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    sample_rate: u32,
    channels: u32,
    max_frames: u32,
    blocks: Vec<Block>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Block {
    samples: Vec<f32>,
    end: bool,
}

pub fn render_ncae(
    path: impl AsRef<Path>,
    input: &[u8],
) -> Result<Value, Box<dyn std::error::Error>> {
    let input: Request = serde_json::from_slice(input)?;
    if input.blocks.len() > 16_384 {
        return Err("too many limiter blocks".into());
    }
    let ncae = ncae::open(path, ncae::Limits::default())?;
    let plan = material_limiter_plan(&serde_json::from_slice(&ncae.plain)?)?;
    let format = Format::new(input.sample_rate as f32, input.channels, input.max_frames)?;
    let mut stage = plan.pcm_stage(format)?;
    let mut block = PlanarBlock::new(format);
    let mut output = Vec::with_capacity(input.blocks.len());
    for next in input.blocks {
        let frames = block.load_interleaved(&next.samples)?;
        let timing = stage.process(&mut block, frames as u32, next.end)?;
        let mut samples = vec![0.0; timing.output_frames as usize * format.channels()];
        block.copy_interleaved(timing.output_frames as usize, &mut samples)?;
        output.push(json!({"timing":timing, "samples":samples}));
    }
    Ok(json!({
        "completeGraph": false,
        "limiterApplied": plan.enabled,
        "lookaheadFrames": stage.lookahead_frames(),
        "blocks": output
    }))
}
