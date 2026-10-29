use super::{Format, PlanarBlock};
use crate::{Error, Result};

/// Recovered loudness node parameters (neaudioeffects A5840/A7AE0).
/// A nonpositive peak disables the peak-based gain ceiling; it is not a limiter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoudnessParameters {
    pub gain_db: f32,
    pub peak: f32,
}

impl Default for LoudnessParameters {
    fn default() -> Self {
        Self {
            gain_db: 0.0,
            peak: 1.0,
        }
    }
}

/// Owned Rust port of the official planar loudness node, including its SIMD
/// group/tail transition semantics. Does not change the user's volume setting.
pub struct LoudnessStage {
    format: Format,
    parameters: LoudnessParameters,
    reference_db: f32,
    bypass: bool,
    target: f32,
    previous: f32,
}

impl LoudnessStage {
    pub fn new(format: Format) -> Self {
        Self {
            format,
            parameters: LoudnessParameters::default(),
            reference_db: -12.0,
            bypass: false,
            target: 1.0,
            previous: 1.0,
        }
    }

    pub fn parameters(&self) -> LoudnessParameters {
        self.parameters
    }
    pub fn reference_db(&self) -> f32 {
        self.reference_db
    }
    pub fn target_factor(&self) -> f32 {
        self.target
    }
    pub fn previous_factor(&self) -> f32 {
        self.previous
    }
    pub fn bypassed(&self) -> bool {
        self.bypass
    }

    fn factor(parameters: LoudnessParameters, reference_db: f32, bypass: bool) -> Result<f32> {
        if !parameters.gain_db.is_finite()
            || !parameters.peak.is_finite()
            || !reference_db.is_finite()
        {
            return Err(Error::InvalidInput("non-finite loudness parameter"));
        }
        if bypass {
            return Ok(1.0);
        }
        // Keep the native f32 operation order; M81's fixed-point conversion is
        // separate from this DSP boundary. The source reference is -18 dB.
        let exponent = ((reference_db + parameters.gain_db) - -18.0) * 0.05;
        let gain = 10.0_f32.powf(exponent);
        let gain = if parameters.peak > 0.0 {
            (1.0 / parameters.peak).min(gain)
        } else {
            gain
        };
        if !gain.is_finite() || exponent.is_nan() {
            return Err(Error::InvalidInput("unrepresentable loudness gain"));
        }
        Ok(gain)
    }

    pub fn set_parameters(&mut self, parameters: LoudnessParameters) -> Result<()> {
        let factor = Self::factor(parameters, self.reference_db, self.bypass)?;
        if parameters != self.parameters {
            self.parameters = parameters;
            self.target = factor;
            self.previous = factor;
        }
        Ok(())
    }

    pub fn set_reference(&mut self, reference_db: f32) -> Result<()> {
        let factor = Self::factor(self.parameters, reference_db, self.bypass)?;
        if reference_db != self.reference_db {
            self.reference_db = reference_db;
            self.target = factor;
            self.previous = factor;
        }
        Ok(())
    }

    pub fn set_bypass(&mut self, bypass: bool) -> Result<()> {
        if bypass != self.bypass {
            let factor = Self::factor(self.parameters, self.reference_db, bypass)?;
            self.bypass = bypass;
            self.target = factor;
        }
        Ok(())
    }

    /// Native A7600 settles a pending transition without touching PCM.
    pub fn reset(&mut self) {
        self.previous = self.target;
    }

    pub fn process(&mut self, block: &mut PlanarBlock, frames: usize) -> Result<()> {
        if block.format() != self.format || frames > self.format.capacity() {
            return Err(Error::InvalidInput("mismatched loudness block"));
        }
        if block.samples().iter().any(|sample| !sample.is_finite()) {
            return Err(Error::InvalidInput("non-finite loudness PCM"));
        }
        if self.previous == 1.0 && self.target == 1.0 {
            return Ok(());
        }
        // Validate the actual gain sequence before mutating either PCM or
        // history. No allocation and no NaN/Inf emission on hostile input.
        for channel in block.samples().chunks_exact(self.format.capacity()) {
            let mut finite = true;
            gains(frames, self.previous, self.target, |i, gain| {
                finite &= (channel[i] * gain).is_finite();
            });
            if !finite {
                return Err(Error::InvalidInput("loudness PCM overflow"));
            }
        }
        for channel in block.samples_mut().chunks_exact_mut(self.format.capacity()) {
            gains(frames, self.previous, self.target, |i, gain| {
                channel[i] *= gain;
            });
        }
        self.previous = self.target;
        Ok(())
    }
}

fn gains(frames: usize, previous: f32, target: f32, mut visit: impl FnMut(usize, f32)) {
    if previous == target {
        for i in 0..frames {
            visit(i, target);
        }
        return;
    }
    let bulk = frames & !3;
    if bulk != 0 {
        let step = (target - previous) / bulk as f32;
        let a = previous + step;
        let b = a + step;
        let mut lanes = [previous, a, b, b + step];
        let advance = step * 4.0;
        for base in (0..bulk).step_by(4) {
            for (lane, gain) in lanes.iter_mut().enumerate() {
                visit(base + lane, *gain);
                *gain += advance;
            }
        }
    }
    if bulk < frames {
        // A6470 restarts the scalar remainder at the previous factor and uses
        // the full valid count as denominator. Do not silently "fix" this edge.
        let step = (target - previous) / frames as f32;
        let mut gain = previous;
        for i in bulk..frames {
            visit(i, gain);
            gain += step;
        }
    }
}
