//! Explicit fixed-bank1 child renderer; no material, service or DLL at runtime.
use cumulus_core::{Error, Result, effects::dsp::*};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    sample_rate: u32,
    max_frames: u32,
    blocks: Vec<Block>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Block {
    samples: Vec<f32>,
    #[serde(default)]
    reset: bool,
    #[serde(default)]
    end: bool,
}

pub fn render(bytes: &[u8]) -> Result<Value> {
    let request: Request = serde_json::from_slice(bytes)?;
    if request.blocks.is_empty()
        || request.blocks.len() > 4096
        || request
            .blocks
            .iter()
            .map(|b| b.samples.len())
            .sum::<usize>()
            > 1_048_576
    {
        return Err(Error::InvalidInput("oversized environment PCM request"));
    }
    let format = Format::new(request.sample_rate as f32, 2, request.max_frames)?;
    let mut environment = AmbienceOneEnvironment::new(format)?;
    let mut block = PlanarBlock::new(format);
    let mut results = Vec::with_capacity(request.blocks.len());
    for input in request.blocks {
        let frames = block.load_interleaved(&input.samples)?;
        if input.reset {
            environment.reset()?;
        }
        let result = environment.process(&mut block, frames, input.end)?;
        let mut samples = vec![0.0; result.valid_frames * 2];
        block.copy_interleaved(result.valid_frames, &mut samples)?;
        results.push(json!({"timing":result,"samples":samples,"state":environment.snapshot()?}));
    }
    Ok(
        json!({"component":"se-bank1-environment","ambience":1,"bank":1,"completeGraph":false,"blocks":results}),
    )
}
