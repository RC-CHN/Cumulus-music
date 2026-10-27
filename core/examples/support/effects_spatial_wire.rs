//! Offline bounded probe protocol; actual processing uses the public Rust API.
use cumulus_core::{Error, Result, effects::dsp::*};
use std::io::{Cursor, Read};

fn word(input: &mut Cursor<&[u8]>) -> Result<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
fn float(input: &mut Cursor<&[u8]>) -> Result<f32> {
    Ok(f32::from_bits(word(input)?))
}
fn floats(output: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        output.extend(value.to_le_bytes());
    }
}
fn parameters(input: &mut Cursor<&[u8]>) -> Result<DynamicsParameters> {
    let mut raw = [0; 171];
    input.read_exact(&mut raw)?;
    DynamicsParameters::from_le_bytes(&raw)
}

pub fn render(mode: &str, data: &[u8], responses: Option<&[u8]>) -> Result<Vec<u8>> {
    if data.len() > 32 * 1024 * 1024 {
        return Err(Error::InvalidInput("oversized spatial probe"));
    }
    let mut input = Cursor::new(data);
    let mut output = Vec::new();
    match mode {
        "kernel" | "stage" => {
            let blocks = word(&mut input)?;
            let phase = float(&mut input)?;
            let capacity = if mode == "stage" {
                word(&mut input)?
            } else {
                256
            };
            if blocks > 4096
                || u64::from(blocks) * (u64::from(capacity) * 8 + 8220) > 32 * 1024 * 1024
            {
                return Err(Error::InvalidInput("oversized rotation probe output"));
            }
            let responses = RotationResponses::from_le_bytes(
                responses.ok_or(Error::InvalidInput("missing rotation responses"))?,
            )?;
            let format = Format::new(44100.0, 2, capacity)?;
            let mut kernel = if mode == "kernel" {
                Some(RotationKernel::new(responses.clone(), phase)?)
            } else {
                None
            };
            let mut stage = if mode == "stage" {
                if phase != 0.0 {
                    return Err(Error::InvalidInput("stage probe starts at zero phase"));
                }
                Some(RotationStage::new(format, responses)?)
            } else {
                None
            };
            let mut block = PlanarBlock::new(format);
            for _ in 0..blocks {
                let frames = word(&mut input)? as usize;
                let period = float(&mut input)?;
                let flags = word(&mut input)?;
                for value in block.samples_mut() {
                    *value = float(&mut input)?;
                }
                if let Some(kernel) = &mut kernel {
                    if flags > 1 {
                        return Err(Error::InvalidInput("invalid rotation reset flag"));
                    }
                    if flags != 0 {
                        kernel.reset();
                    }
                    kernel.set_period(period)?;
                    let valid = kernel.process(&mut block, frames)?;
                    output.extend((valid as u32).to_le_bytes());
                    output.extend(kernel.phase().to_le_bytes());
                    floats(&mut output, block.samples());
                    floats(&mut output, kernel.interpolated_responses());
                } else if let Some(stage) = &mut stage {
                    if flags > 7 {
                        return Err(Error::InvalidInput("invalid rotation flags"));
                    }
                    if flags & 4 != 0 {
                        stage.reset();
                    }
                    stage.set_period(period)?;
                    stage.set_bypass(flags & 1 != 0)?;
                    let result = stage.process(&mut block, frames, flags & 2 != 0)?;
                    output.extend((result.valid_frames as u32).to_le_bytes());
                    output.extend(u32::from(result.end).to_le_bytes());
                    output.extend(result.phase.to_le_bytes());
                    floats(&mut output, block.samples());
                }
            }
        }
        "internal" | "sidechain" => {
            let (rate, capacity, blocks) =
                (word(&mut input)?, word(&mut input)?, word(&mut input)?);
            let format = Format::new(rate as f32, 1, capacity)?;
            if blocks > 4096
                || u64::from(blocks) * (u64::from(capacity) * 8 + 40) > 32 * 1024 * 1024
            {
                return Err(Error::InvalidInput("oversized dynamics probe output"));
            }
            let mut processor = MultibandDynamics::new(format, &parameters(&mut input)?)?;
            let (mut main, mut side) = (PlanarBlock::new(format), PlanarBlock::new(format));
            for _ in 0..blocks {
                let (frames, flags) = (word(&mut input)? as usize, word(&mut input)?);
                if flags > 3 {
                    return Err(Error::InvalidInput("invalid dynamics flags"));
                }
                if flags & 2 != 0 {
                    processor.configure(&parameters(&mut input)?)?;
                }
                if flags & 1 != 0 {
                    processor.reset()?;
                }
                for value in main.samples_mut().iter_mut().chain(side.samples_mut()) {
                    *value = float(&mut input)?;
                }
                processor.process(
                    &mut main,
                    if mode == "sidechain" {
                        Some(&mut side)
                    } else {
                        None
                    },
                    frames,
                )?;
                floats(&mut output, main.samples());
                floats(&mut output, side.samples());
                floats(&mut output, &processor.detector_history()?);
            }
        }
        _ => return Err(Error::InvalidInput("unknown spatial probe mode")),
    }
    if input.position() != data.len() as u64 {
        return Err(Error::InvalidInput("trailing spatial probe input"));
    }
    Ok(output)
}
