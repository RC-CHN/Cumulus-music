use super::{
    ffi,
    state::{State, status},
};
use crate::{Error, Result};

#[derive(Clone, Copy, Debug)]
#[repr(u32)]
pub enum GraphicKind {
    LowShelf,
    Peak,
    HighShelf,
    LowPass,
    HighPass,
    BandPass,
    Notch,
    LowShelfQ,
    HighShelfQ,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum ParametricKind {
    LowPass,
    HighPass,
    BandPass,
    Notch,
    LowShelf,
    HighShelf,
    Peak,
    LowShelfQ,
    HighShelfQ,
    LowPassQ,
    HighPassQ,
}

impl TryFrom<u32> for ParametricKind {
    type Error = Error;
    fn try_from(value: u32) -> Result<Self> {
        use ParametricKind::*;
        [
            LowPass, HighPass, BandPass, Notch, LowShelf, HighShelf, Peak, LowShelfQ, HighShelfQ,
            LowPassQ, HighPassQ,
        ]
        .get(value as usize)
        .copied()
        .ok_or(Error::InvalidInput("unknown PEQ filter type"))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Coefficients([f32; 5]);

impl Coefficients {
    pub fn graphic(
        kind: GraphicKind,
        rate: f32,
        frequency: f32,
        gain: f32,
        q: f32,
    ) -> Result<Self> {
        let mut result = [0.0; 5];
        // SAFETY: five writable floats. C validates every scalar before use.
        status(unsafe {
            ffi::netease_biquad_coefficients(
                kind as u32,
                rate,
                frequency,
                gain,
                q,
                result.as_mut_ptr(),
            )
        })?;
        Ok(Self(result))
    }
    pub fn parametric(
        kind: ParametricKind,
        rate: f32,
        frequency: f32,
        gain: f32,
        q: f32,
    ) -> Result<Self> {
        let mut result = [0.0; 5];
        // SAFETY: same output contract, separate official parameter-EQ enum.
        status(unsafe {
            ffi::netease_peq_coefficients(
                kind as u32,
                rate,
                frequency,
                gain,
                q,
                result.as_mut_ptr(),
            )
        })?;
        Ok(Self(result))
    }
    pub fn values(self) -> [f32; 5] {
        self.0
    }
}

pub struct Biquad {
    state: State,
    failed: bool,
}

impl Biquad {
    pub fn new(coefficients: Coefficients) -> Result<Self> {
        let mut value = Self {
            state: State::new(0)?,
            failed: false,
        };
        value.set(coefficients);
        Ok(value)
    }
    /// Reconfiguration preserves sample history, as the official setter does.
    pub fn set(&mut self, coefficients: Coefficients) {
        // SAFETY: exact native allocation, finite coefficients created above.
        unsafe { ffi::netease_biquad_set(self.state.pointer(), coefficients.0.as_ptr()) };
    }
    pub fn reset(&mut self) {
        // SAFETY: initialized state; reset clears only the four history floats.
        unsafe { ffi::netease_biquad_reset(self.state.pointer()) };
        self.failed = false;
    }
    pub fn process(&mut self, samples: &mut [f32]) -> Result<()> {
        if self.failed || samples.iter().any(|value| !value.is_finite()) {
            return Err(Error::InvalidInput("invalid or failed biquad stream"));
        }
        if samples.is_empty() {
            return Ok(());
        }
        // SAFETY: C supports any float alignment and exactly this slice length.
        unsafe {
            ffi::netease_biquad_process(self.state.pointer(), samples.as_mut_ptr(), samples.len())
        };
        if samples.iter().any(|value| !value.is_finite()) {
            samples.fill(0.0);
            self.failed = true;
            return Err(Error::Media("non-finite biquad output"));
        }
        Ok(())
    }
}
