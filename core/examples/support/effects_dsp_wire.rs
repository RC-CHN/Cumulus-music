//! Bounded local probe protocol shared by the executable and frozen tests.
//! Production processing lives in core::effects::dsp, not in this decoder.
use cumulus_core::{Error, Result, effects::dsp::*};
use std::io::{Cursor, Read};

fn u32_value(input: &mut Cursor<&[u8]>) -> Result<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
fn float(input: &mut Cursor<&[u8]>) -> Result<f32> {
    Ok(f32::from_bits(u32_value(input)?))
}
fn boolean(input: &mut Cursor<&[u8]>) -> Result<bool> {
    match u32_value(input)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::InvalidInput("invalid probe boolean")),
    }
}

fn graphic_kind(value: u32) -> Result<GraphicKind> {
    use GraphicKind::*;
    [
        LowShelf, Peak, HighShelf, LowPass, HighPass, BandPass, Notch, LowShelfQ, HighShelfQ,
    ]
    .get(value as usize)
    .copied()
    .ok_or(Error::InvalidInput("invalid probe filter kind"))
}

enum Processor {
    Graphic(GraphicEq),
    Bass(BassTreble),
    Parametric(ParametricEq),
}

/// Explicit component selection, never an inferred full preset effect graph.
pub fn render_peq_material(
    material: &serde_json::Value,
    section: &str,
    bytes: &[u8],
) -> Result<Vec<f32>> {
    use cumulus_core::effects::material::{apply_peq, peq_calls, peq_sections};
    let fragment = peq_sections(material)?
        .into_iter()
        .find(|(name, _)| *name == section)
        .ok_or(Error::InvalidInput("missing requested PEQ fragment"))?
        .1;
    let calls = peq_calls(fragment)?;
    let mut input = Cursor::new(bytes);
    let (rate, channels, capacity, blocks) = (
        u32_value(&mut input)?,
        u32_value(&mut input)?,
        u32_value(&mut input)?,
        u32_value(&mut input)?,
    );
    let format = Format::new(rate as f32, channels, capacity)?;
    if blocks > 4096 || format.sample_count() * blocks as usize > 4 * 1024 * 1024 {
        return Err(Error::InvalidInput("oversized material probe output"));
    }
    let mut stage = ParametricEq::new(format, false, true)?;
    apply_peq(&mut stage, &calls)?;
    let mut output = Vec::new();
    let mut block = PlanarBlock::new(format);
    for _ in 0..blocks {
        let frames = u32_value(&mut input)? as usize;
        for value in block.samples_mut() {
            *value = float(&mut input)?;
        }
        stage.process(&mut block, frames)?;
        output.extend_from_slice(block.samples());
    }
    if input.position() != bytes.len() as u64 {
        return Err(Error::InvalidInput("trailing material probe input"));
    }
    Ok(output)
}

pub fn render(mode: &str, bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(Error::InvalidInput("oversized DSP probe"));
    }
    let mut input = Cursor::new(bytes);
    let mut output = Vec::new();
    match mode {
        "coeff" => {
            let kind = graphic_kind(u32_value(&mut input)?)?;
            let (rate, frequency, gain, q) = (
                float(&mut input)?,
                float(&mut input)?,
                float(&mut input)?,
                float(&mut input)?,
            );
            output.extend(Coefficients::graphic(kind, rate, frequency, gain, q)?.values());
        }
        "peq-coeff" => {
            let kind = ParametricKind::try_from(u32_value(&mut input)?)?;
            let rate = u32_value(&mut input)? as f32;
            let (gain, frequency, q) = (float(&mut input)?, float(&mut input)?, float(&mut input)?);
            output.extend(Coefficients::parametric(kind, rate, frequency, gain, q)?.values());
        }
        "biquad" => {
            let kind = graphic_kind(u32_value(&mut input)?)?;
            let (rate, frequency, gain, q) = (
                float(&mut input)?,
                float(&mut input)?,
                float(&mut input)?,
                float(&mut input)?,
            );
            let mut filter = Biquad::new(Coefficients::graphic(kind, rate, frequency, gain, q)?)?;
            let blocks = u32_value(&mut input)?;
            if blocks > 4096 {
                return Err(Error::InvalidInput("too many probe blocks"));
            }
            let mut block = PlanarBlock::new(Format::new(rate, 1, 4096)?);
            for _ in 0..blocks {
                let action = u32_value(&mut input)?;
                let frames = u32_value(&mut input)? as usize;
                let offset = u32_value(&mut input)? as usize;
                if frames > 4092 || offset > 3 {
                    return Err(Error::InvalidInput("invalid biquad probe size"));
                }
                match action {
                    0 => {}
                    1 => {
                        let (frequency, gain, q) =
                            (float(&mut input)?, float(&mut input)?, float(&mut input)?);
                        filter.set(Coefficients::graphic(kind, rate, frequency, gain, q)?);
                    }
                    2 => filter.reset(),
                    _ => return Err(Error::InvalidInput("invalid biquad probe action")),
                }
                let samples = &mut block.samples_mut()[offset..offset + frames];
                for value in &mut *samples {
                    *value = float(&mut input)?;
                }
                filter.process(samples)?;
                output.extend_from_slice(samples);
            }
        }
        "bt" | "geq-outer" | "peq" => {
            let (rate, channels, capacity, blocks) = (
                u32_value(&mut input)?,
                u32_value(&mut input)?,
                u32_value(&mut input)?,
                u32_value(&mut input)?,
            );
            if blocks > 4096 {
                return Err(Error::InvalidInput("too many probe blocks"));
            }
            let format = Format::new(rate as f32, channels, capacity)?;
            if format.sample_count() * blocks as usize > 4 * 1024 * 1024 {
                return Err(Error::InvalidInput("oversized probe output"));
            }
            let mut processor = match mode {
                "bt" => Processor::Bass(BassTreble::new(format)?),
                "geq-outer" => Processor::Graphic(GraphicEq::new(format)?),
                _ => {
                    let config = u32_value(&mut input)?;
                    if config & !0x8000 != channels {
                        return Err(Error::InvalidInput("invalid probe LFE layout"));
                    }
                    Processor::Parametric(ParametricEq::new(
                        format,
                        config & 0x8000 != 0,
                        boolean(&mut input)?,
                    )?)
                }
            };
            let mut block = PlanarBlock::new(format);
            for _ in 0..blocks {
                let frames = u32_value(&mut input)? as usize;
                let bypass = boolean(&mut input)?;
                match &mut processor {
                    Processor::Bass(stage) => {
                        stage.configure(BassTrebleSettings {
                            bass_db: float(&mut input)?,
                            treble_db: float(&mut input)?,
                            low_frequency: float(&mut input)?,
                            low_q: float(&mut input)?,
                            high_frequency: float(&mut input)?,
                            high_q: float(&mut input)?,
                        })?;
                        stage.set_bypass(bypass)?;
                    }
                    Processor::Graphic(stage) => {
                        let mut gains = [0.0; 10];
                        for value in &mut gains {
                            *value = float(&mut input)?;
                        }
                        stage.set_gains(&gains)?;
                        stage.set_bypass(bypass)?;
                    }
                    Processor::Parametric(stage) => {
                        stage.set_gain(float(&mut input)?)?;
                        stage.set_process_lfe(boolean(&mut input)?)?;
                        for index in 0..9 {
                            stage.set_band(
                                index,
                                ParametricBand {
                                    kind: ParametricKind::try_from(u32_value(&mut input)?)?,
                                    gain_db: float(&mut input)?,
                                    frequency: float(&mut input)?,
                                    q: float(&mut input)?,
                                    enabled: boolean(&mut input)?,
                                },
                            )?;
                        }
                        stage.set_bypass(bypass)?;
                    }
                }
                for value in block.samples_mut() {
                    *value = float(&mut input)?;
                }
                match &mut processor {
                    Processor::Bass(stage) => stage.process(&mut block, frames)?,
                    Processor::Graphic(stage) => stage.process(&mut block, frames)?,
                    Processor::Parametric(stage) => stage.process(&mut block, frames)?,
                }
                output.extend_from_slice(block.samples());
            }
        }
        _ => return Err(Error::InvalidInput("unknown DSP probe mode")),
    }
    if input.position() != bytes.len() as u64 {
        return Err(Error::InvalidInput("trailing DSP probe input"));
    }
    Ok(output)
}
