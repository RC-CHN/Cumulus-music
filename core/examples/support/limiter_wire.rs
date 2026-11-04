use cumulus_core::effects::{
    dsp::{Format, PlanarBlock},
    material::{MaterialLimiterPlan, material_limiter_plan},
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
    let plan = load_plan(path)?;
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

fn load_plan(path: impl AsRef<Path>) -> Result<MaterialLimiterPlan, Box<dyn std::error::Error>> {
    let ncae = ncae::open(path, ncae::Limits::default())?;
    Ok(material_limiter_plan(&serde_json::from_slice(
        &ncae.plain,
    )?)?)
}

pub fn stream_ncae(
    path: impl AsRef<Path>,
    input: &[u8],
) -> Result<Value, Box<dyn std::error::Error>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct StreamRequest {
        sample_rate: u32,
        channels: u32,
        #[serde(default = "native_quantum")]
        quantum: u32,
        fifo_capacity: u32,
        operations: Vec<Operation>,
    }

    #[derive(Deserialize)]
    #[serde(
        tag = "op",
        rename_all = "camelCase",
        rename_all_fields = "camelCase",
        deny_unknown_fields
    )]
    enum Operation {
        Push {
            samples: Vec<f32>,
            timestamp_ms: Option<u32>,
        },
        Pull {
            capacity: u32,
        },
    }

    fn native_quantum() -> u32 {
        2048
    }

    let input: StreamRequest = serde_json::from_slice(input)?;
    if input.operations.len() > 16_384 {
        return Err("too many limiter stream operations".into());
    }
    let plan = load_plan(path)?;
    let format = Format::new(input.sample_rate as f32, input.channels, input.quantum)?;
    let mut stream = plan.pcm_stream(format, input.fifo_capacity)?;
    let mut output = Vec::with_capacity(input.operations.len());
    for operation in input.operations {
        match operation {
            Operation::Push {
                samples,
                timestamp_ms,
            } => {
                let mut quanta = Vec::new();
                let status =
                    stream.push_interleaved_observed(&samples, timestamp_ms, |quantum| {
                        quanta.push(quantum)
                    })?;
                output.push(json!({"op":"push", "status":status, "quanta":quanta}));
            }
            Operation::Pull { capacity } => {
                if capacity > 65_536 {
                    return Err("limiter stream read capacity is too large".into());
                }
                let channels = format.channels();
                let mut samples = vec![0.0; capacity as usize * channels];
                let status = stream.pull_interleaved(&mut samples)?;
                samples.truncate(status.frames as usize * channels);
                output.push(json!({"op":"pull", "status":status, "samples":samples}));
            }
        }
    }
    Ok(json!({
        "completeGraph": false,
        "limiterApplied": plan.enabled,
        "lookaheadFrames": stream.lookahead_frames(),
        "quantum": input.quantum,
        "operations": output,
    }))
}
