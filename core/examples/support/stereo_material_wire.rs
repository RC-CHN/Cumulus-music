//! Explicit source-backed subgraph/transport tool, never a remote material grant.
use cumulus_core::{
    Error, Result,
    effects::{
        dsp::{Format, PlanarBlock},
        stereo::{LocalStereoMaterialPlan, LocalStereoStream, LocalStereoSubgraph},
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    sample_rate: u32,
    max_frames: u32,
    material: Value,
    blocks: Vec<Block>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Block {
    samples: Vec<f32>,
    #[serde(default)]
    end: bool,
    #[serde(default)]
    reset: bool,
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
        return Err(Error::InvalidInput("oversized local stereo PCM request"));
    }
    let plan = LocalStereoMaterialPlan::from_equalizer_json(&request.material)?;
    let format = Format::new(request.sample_rate as f32, 2, request.max_frames)?;
    if request.blocks.len().saturating_mul(format.sample_count()) > 1_048_576 {
        return Err(Error::InvalidInput("oversized local stereo output request"));
    }
    let mut graph = LocalStereoSubgraph::new(format, plan)?;
    let mut block = PlanarBlock::new(format);
    let mut blocks = Vec::with_capacity(request.blocks.len());
    for input in request.blocks {
        let frames = block.load_interleaved(&input.samples)?;
        if input.reset {
            graph.reset()?;
        }
        let timing = graph.process(&mut block, frames, input.end)?;
        let mut samples = vec![0.0; 2 * timing.valid_frames];
        block.copy_interleaved(timing.valid_frames, &mut samples)?;
        blocks.push(json!({"timing":timing,"samples":samples}));
    }
    Ok(
        json!({"component":"local-se1-default-limiter-subgraph","fullSdkGraph":false,"completeGraph":false,"blocks":blocks}),
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StreamRequest {
    sample_rate: u32,
    decoded_channels: u32,
    fifo_capacity: u32,
    material: Value,
    actions: Vec<Action>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Push {
        samples: Vec<f32>,
        #[serde(rename = "timestampMs")]
        timestamp_ms: Option<u32>,
    },
    Read {
        frames: u32,
    },
    Reset,
}
pub fn stream(bytes: &[u8]) -> Result<Value> {
    let request: StreamRequest = serde_json::from_slice(bytes)?;
    let mut input = 0usize;
    let mut output = 0usize;
    for action in &request.actions {
        match action {
            Action::Push { samples, .. } => input = input.saturating_add(samples.len()),
            Action::Read { frames } => output = output.saturating_add(*frames as usize * 2),
            Action::Reset => (),
        }
    }
    if request.actions.is_empty()
        || request.actions.len() > 4096
        || input > 1_048_576
        || output > 1_048_576
    {
        return Err(Error::InvalidInput(
            "oversized local stereo transport request",
        ));
    }
    let plan = LocalStereoMaterialPlan::from_equalizer_json(&request.material)?;
    let mut stream = LocalStereoStream::new(
        request.sample_rate,
        request.decoded_channels,
        plan,
        request.fifo_capacity,
    )?;
    let mut actions = Vec::with_capacity(request.actions.len());
    for action in request.actions {
        match action {
            Action::Push {
                samples,
                timestamp_ms,
            } => actions.push(
                json!({"kind":"push","status":stream.push_interleaved(&samples,timestamp_ms)?}),
            ),
            Action::Read { frames } => {
                let mut samples = vec![0.0; 2 * frames as usize];
                let status = stream.pull_interleaved(&mut samples)?;
                samples.truncate(2 * status.frames as usize);
                actions.push(json!({"kind":"read","status":status,"samples":samples}));
            }
            Action::Reset => {
                stream.reset()?;
                actions.push(json!({"kind":"reset"}));
            }
        }
    }
    Ok(
        json!({"component":"local-se1-default-limiter-stream","fullSdkGraph":false,"completeGraph":false,"actions":actions}),
    )
}
