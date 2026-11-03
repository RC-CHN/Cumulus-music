//! Rotate3D resampler setup and output capacity from neaudioeffects.dll
//! RVAs 23480, 78770, 779c0 and 788c0. No SoXR engine is loaded here.
use crate::{Error, Result};

pub const ROTATION_KERNEL_RATE: u32 = 44_100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RotationRateConversion {
    input_rate: u32,
    output_rate: u32,
}

impl RotationRateConversion {
    pub fn input_rate(self) -> u32 {
        self.input_rate
    }

    pub fn output_rate(self) -> u32 {
        self.output_rate
    }

    /// Both input and output are stereo, split planar float32
    /// (SoXR datatype 4), with unit scale and default quality/runtime.
    pub const CHANNELS: u32 = 2;
    pub const SOXR_DATATYPE: u32 = 4;
    pub const SOXR_SCALE: f64 = 1.0;

    /// Original wrapper's output frame bound. The delay argument is in
    /// output frames as reported by SoXR, not milliseconds or input frames.
    pub fn output_capacity(self, input_frames: u32, delay: f64) -> Result<u32> {
        if input_frames > 4096 || !delay.is_finite() || delay < 0.0 {
            return Err(Error::InvalidInput("invalid rotation resampler extent"));
        }
        // Preserve the original binary64 evaluation order at half-frame edges.
        let ratio = f64::from(self.output_rate) / f64::from(self.input_rate);
        let rounded = ((delay + ratio * f64::from(input_frames)) + 1.0) + 0.5;
        if rounded >= f64::from(u32::MAX) + 1.0 {
            return Err(Error::InvalidInput("rotation resampler capacity overflow"));
        }
        Ok(rounded as u32)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RotationRatePlan {
    Direct,
    Convert {
        ingress: RotationRateConversion,
        egress: RotationRateConversion,
    },
}

impl RotationRatePlan {
    pub fn new(source_rate: u32) -> Result<Self> {
        if !(8_000..=384_000).contains(&source_rate) {
            return Err(Error::InvalidInput("unsupported rotation source rate"));
        }
        if source_rate == ROTATION_KERNEL_RATE {
            return Ok(Self::Direct);
        }
        Ok(Self::Convert {
            ingress: RotationRateConversion {
                input_rate: source_rate,
                output_rate: ROTATION_KERNEL_RATE,
            },
            egress: RotationRateConversion {
                input_rate: ROTATION_KERNEL_RATE,
                output_rate: source_rate,
            },
        })
    }
}
