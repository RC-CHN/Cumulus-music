//! Direct NCAE component workflow: bounded stereo f32le input/output.
//! The native component determines latency and flush length; no trimming.
use cumulus_core::{
    Error, Result,
    effects::{
        dsp::{Format, PlanarBlock, RotationResponses, RotationResult, RotationStage},
        material::{apply_rotation, rotation_calls, spatial_sections},
    },
};
use serde_json::Value;

fn section(material: &Value) -> Result<&Value> {
    spatial_sections(material)?
        .into_iter()
        .find(|(name, _)| matches!(*name, "rotate" | "aev2.rotate"))
        .map(|(_, value)| value)
        .ok_or(Error::InvalidInput(
            "material has no selected rotation section",
        ))
}

pub fn render(material: &Value, responses: &[u8], input: &[u8]) -> Result<Vec<u8>> {
    let format = Format::new(44100.0, 2, 1024)?;
    let mut stage = RotationStage::new(format, RotationResponses::from_le_bytes(responses)?)?;
    apply_rotation(&mut stage, &rotation_calls(section(material)?)?)?;
    render_pcm(format, input, |block, count, end| {
        stage.process(block, count, end)
    })
}

#[cfg(target_os = "linux")]
pub fn render_resampled(
    material: &Value,
    responses: &[u8],
    input: &[u8],
    rate: u32,
    host: std::sync::Arc<super::soxr::Host>,
) -> Result<Vec<u8>> {
    use cumulus_core::effects::{dsp::ResampledRotationStage, material::RotationCall};
    if !matches!(rate, 48_000 | 96_000) {
        return Err(Error::InvalidInput(
            "resampled rotation requires 48000 or 96000 Hz",
        ));
    }
    let format = Format::new(rate as f32, 2, 1024)?;
    let calls = rotation_calls(section(material)?)?;
    let mut stage = ResampledRotationStage::new(
        format,
        RotationResponses::from_le_bytes(responses)?,
        |rate| super::soxr::Converter::new(host.clone(), rate),
    )?;
    for call in calls {
        match call {
            RotationCall::Velocity { value } => {
                stage.set_period(value)?;
            }
            RotationCall::On { value } => stage.set_bypass(!value),
        }
    }
    render_pcm(format, input, |block, count, end| {
        stage.process(block, count, end)
    })
}

fn render_pcm(
    format: Format,
    input: &[u8],
    mut process: impl FnMut(&mut PlanarBlock, usize, bool) -> Result<RotationResult>,
) -> Result<Vec<u8>> {
    if input.len() > 32 * 1024 * 1024 || !input.len().is_multiple_of(8) {
        return Err(Error::InvalidInput("expected bounded stereo f32le input"));
    }
    let pcm: Vec<_> = input
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().expect("four bytes")))
        .collect();
    if pcm.iter().any(|value| !value.is_finite()) {
        return Err(Error::InvalidInput("non-finite input PCM"));
    }
    let mut block = PlanarBlock::new(format);
    let mut interleaved = vec![0.0; format.sample_count()];
    let mut output = Vec::new();
    for samples in pcm.chunks(format.sample_count()) {
        let count = block.load_interleaved(samples)?;
        let result = process(&mut block, count, false)?;
        append(&block, result.valid_frames, &mut interleaved, &mut output)?;
    }
    // The bounded native FIFO has fewer than 9K frames. A larger guard detects
    // a broken drain contract instead of looping indefinitely.
    for _ in 0..32 {
        block.samples_mut().fill(0.0);
        let result = process(&mut block, 0, true)?;
        append(&block, result.valid_frames, &mut interleaved, &mut output)?;
        if result.end {
            return Ok(output);
        }
    }
    Err(Error::State("rotation drain did not finish"))
}

fn append(
    block: &PlanarBlock,
    frames: usize,
    interleaved: &mut [f32],
    output: &mut Vec<u8>,
) -> Result<()> {
    let samples = &mut interleaved[..frames * 2];
    block.copy_interleaved(frames, samples)?;
    if output.len() + samples.len() * 4 > 32 * 1024 * 1024 + 8192 {
        return Err(Error::State("rotation output exceeded bounded stream"));
    }
    for sample in samples {
        output.extend(sample.to_le_bytes());
    }
    Ok(())
}
