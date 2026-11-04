use super::Format;
use crate::{Error, Result};
use serde::Serialize;

/// Frame and EOF contract only. This does not buffer, produce, or modify PCM.
#[derive(Debug)]
pub struct LimiterFrameFlow {
    max_frames: u32,
    lookahead_frames: u32,
    skip: u32,
    drain: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimiterBlockFlow {
    pub output_frames: u32,
    pub output_end: bool,
    pub pad_input_frames: u32,
    pub skip_output_frames: u32,
}

impl LimiterFrameFlow {
    pub(crate) fn new(format: Format, enabled: bool, attack: f32) -> Result<Self> {
        if format.rate().fract() != 0.0 {
            return Err(Error::InvalidInput(
                "limiter requires an integer sample rate",
            ));
        }
        let lookahead = if enabled {
            // The node's allocation at 64F83–64F97 is sized for 100 ms.
            if !attack.is_finite() || attack <= 0.0 || attack > 0.1 {
                return Err(Error::InvalidInput("unsupported limiter attack window"));
            }
            // 6501F–6504C: truncate attack * rate * channels, then align to a
            // full frame. 6533F–65351 seeds both skip and drain to frames-1.
            let channels = format.channels() as u32;
            let samples =
                (f64::from(format.rate()) * f64::from(attack) * f64::from(channels)) as u32;
            (samples / channels)
                .checked_sub(1)
                .ok_or(Error::InvalidInput(
                    "limiter attack is shorter than one frame",
                ))?
        } else {
            0
        };
        Ok(Self {
            max_frames: format.capacity() as u32,
            lookahead_frames: lookahead,
            skip: lookahead,
            drain: lookahead,
        })
    }

    pub fn lookahead_frames(&self) -> u32 {
        self.lookahead_frames
    }

    pub fn next_block(&mut self, input_frames: u32, input_end: bool) -> Result<LimiterBlockFlow> {
        if input_frames > self.max_frames {
            return Err(crate::Error::InvalidInput(
                "limiter input exceeds block capacity",
            ));
        }
        // 63C2E–63C77 extends EOF input within the current block capacity.
        let padding = if input_end {
            (self.max_frames - input_frames).min(self.drain)
        } else {
            0
        };
        self.drain -= padding;
        let processed = input_frames + padding;
        // 64B55–64BB0 withholds the initial lookahead frames, which can
        // span multiple input blocks or even the first EOF blocks.
        let skip = processed.min(self.skip);
        self.skip -= skip;
        Ok(LimiterBlockFlow {
            output_frames: processed - skip,
            output_end: input_end && self.drain == 0,
            pad_input_frames: padding,
            skip_output_frames: skip,
        })
    }
}
