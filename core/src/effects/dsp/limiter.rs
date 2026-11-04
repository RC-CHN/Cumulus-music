//! Bounded fresh-stream PCM path from the recovered x64 lookahead limiter.
//! Live reconfiguration and the full native material graph are separate.
mod envelope;
mod flow;
mod stream;

use super::{Format, PlanarBlock};
use crate::{Error, Result};
use envelope::Envelope;
pub use flow::{LimiterBlockFlow, LimiterFrameFlow};
pub use stream::{LimiterStream, LimiterStreamQuantum, LimiterStreamRead, LimiterStreamWrite};

#[derive(Clone, Copy, Debug)]
pub struct LimiterSettings {
    pub enabled: bool,
    pub threshold_db: f32,
    pub attack_seconds: f32,
    pub release_seconds: f32,
    pub input_gain_db: f32,
    pub output_gain_db: f32,
    pub infinite_ratio: bool,
    pub ratio: f32,
}

pub struct LimiterStage {
    format: Format,
    flow: LimiterFrameFlow,
    envelope: Option<Envelope>,
}

impl LimiterStage {
    /// Open a fresh node with known controls. Rejects non-finite or unsupported
    /// control values before creating any sample history.
    pub fn new(format: Format, settings: LimiterSettings) -> Result<Self> {
        let flow = LimiterFrameFlow::new(format, settings.enabled, settings.attack_seconds)?;
        let envelope = if settings.enabled {
            // The offline wrapper deliberately bounds dB conversion and
            // release duration; unsupported official cases are not guessed.
            let gains = [
                settings.threshold_db,
                settings.input_gain_db,
                settings.output_gain_db,
            ];
            if gains.iter().any(|gain| !(-120.0..=120.0).contains(gain))
                || !(0.000_001..=10.0).contains(&settings.release_seconds)
                || !(0.125..=100.0).contains(&settings.ratio)
                || flow.lookahead_frames() == 0
            {
                return Err(Error::InvalidInput("unsupported limiter PCM settings"));
            }
            Some(Envelope::new(
                format,
                flow.lookahead_frames() as usize + 1,
                settings,
            ))
        } else {
            None
        };
        Ok(Self {
            format,
            flow,
            envelope,
        })
    }

    pub fn lookahead_frames(&self) -> u32 {
        self.flow.lookahead_frames()
    }

    /// Replace the beginning of the planar block with up to its capacity of
    /// output frames. At EOF, pad with zeros only as required by the recovered
    /// frame contract; call again until output_end is true.
    pub fn process(
        &mut self,
        block: &mut PlanarBlock,
        frames: u32,
        end: bool,
    ) -> Result<LimiterBlockFlow> {
        let capacity = self.format.capacity();
        let channels = self.format.channels();
        if block.format() != self.format || frames as usize > capacity {
            return Err(Error::InvalidInput(
                "invalid limiter PCM block format or frame count",
            ));
        }
        if block.samples().chunks_exact(capacity).any(|channel| {
            channel[..frames as usize]
                .iter()
                .any(|sample| !sample.is_finite())
        }) {
            return Err(Error::InvalidInput("non-finite limiter PCM input"));
        }
        let timing = self.flow.next_block(frames, end)?;
        let Some(envelope) = self.envelope.as_mut() else {
            return Ok(timing);
        };
        let mut input = [0.0; 8];
        let mut output = [0.0; 8];
        let input_frames = frames as usize;
        let skip = timing.skip_output_frames as usize;
        let samples = block.samples_mut();
        for frame in 0..input_frames + timing.pad_input_frames as usize {
            for channel in 0..channels {
                input[channel] = if frame < input_frames {
                    samples[channel * capacity + frame]
                } else {
                    0.0
                };
            }
            envelope.step(&input[..channels], &mut output[..channels]);
            // The write index never passes the read index, even during EOF.
            if let Some(written) = frame.checked_sub(skip) {
                for channel in 0..channels {
                    samples[channel * capacity + written] = output[channel] as f32;
                }
            }
        }
        Ok(timing)
    }
}

#[cfg(test)]
mod tests;
