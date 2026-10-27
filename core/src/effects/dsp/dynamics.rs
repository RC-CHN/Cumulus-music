use super::{
    DynamicsParameters, Format, PlanarBlock, ffi,
    state::{State, status},
};
use crate::{Error, Result};

/// The recovered mono dynamics component used separately for SE mid/side.
/// Multichannel linking and full StereoEnhancer processing are not implied.
pub struct MultibandDynamics {
    state: State,
    format: Format,
    failed: bool,
}

impl MultibandDynamics {
    pub fn new(format: Format, parameters: &DynamicsParameters) -> Result<Self> {
        if format.channels() != 1 {
            return Err(Error::InvalidInput(
                "dynamics component requires mono buffers",
            ));
        }
        let mut state = State::new(6)?;
        let raw = parameters.as_le_bytes();
        // SAFETY: matching aligned state, complete serialized block; bridge
        // validates all coefficients before the recovered initializer executes.
        status(unsafe {
            ffi::cumulus_multiband_init(
                state.pointer(),
                format.rate(),
                format.capacity() as u32,
                raw.as_ptr(),
                raw.len(),
            )
        })?;
        Ok(Self {
            state,
            format,
            failed: false,
        })
    }

    /// The crossover order stays fixed at initialization, even if the new
    /// parameter block changes its fourth_order flag. Gain history is retained.
    pub fn configure(&mut self, parameters: &DynamicsParameters) -> Result<()> {
        let raw = parameters.as_le_bytes();
        // SAFETY: complete block; preflight checks the actual initialized order
        // before mutating state, preserving atomic rejection on invalid input.
        status(unsafe { ffi::cumulus_multiband_set(self.state.pointer(), raw.as_ptr(), raw.len()) })
    }

    /// Clear filters/detectors; preserve previous input/output gain values.
    pub fn reset(&mut self) -> Result<()> {
        // SAFETY: exclusive initialized state. This is a partial native reset;
        // numerical failures still require recreating the whole stage.
        status(unsafe { ffi::netease_multiband_reset(self.state.pointer()) })
    }

    pub fn detector_history(&self) -> Result<[f32; 10]> {
        let mut result = [0.0; 10];
        // SAFETY: read-only state and exactly ten writable history floats.
        status(unsafe {
            ffi::netease_multiband_history(self.state.const_pointer(), result.as_mut_ptr())
        })?;
        Ok(result)
    }

    pub fn process(
        &mut self,
        main: &mut PlanarBlock,
        mut side: Option<&mut PlanarBlock>,
        frames: usize,
    ) -> Result<()> {
        if self.failed {
            return Err(Error::State("dynamics failed; recreate the stage"));
        }
        if frames > self.format.capacity()
            || main.format() != self.format
            || side
                .as_ref()
                .is_some_and(|block| block.format() != self.format)
        {
            return Err(Error::InvalidInput(
                "dynamics block format or frame count mismatch",
            ));
        }
        if main.samples().iter().any(|value| !value.is_finite())
            || side
                .as_ref()
                .is_some_and(|block| block.samples().iter().any(|value| !value.is_finite()))
        {
            return Err(Error::InvalidInput("non-finite dynamics input"));
        }
        let pointer = side.as_mut().map_or(std::ptr::null_mut(), |block| {
            block.samples_mut().as_mut_ptr()
        });
        // SAFETY: full-capacity distinct allocations guaranteed by Rust's
        // exclusive borrows; C may zero the complete main buffer and modify
        // valid side samples. Frame bound and both formats checked above.
        status(unsafe {
            ffi::netease_multiband_process(
                self.state.pointer(),
                main.samples_mut().as_mut_ptr(),
                pointer,
                frames,
            )
        })?;
        if main.samples().iter().any(|value| !value.is_finite())
            || side
                .as_ref()
                .is_some_and(|block| block.samples().iter().any(|value| !value.is_finite()))
            || self
                .detector_history()?
                .iter()
                .any(|value| !value.is_finite())
        {
            main.samples_mut().fill(0.0);
            if let Some(side) = side {
                side.samples_mut().fill(0.0);
            }
            self.failed = true;
            return Err(Error::Media("non-finite dynamics output"));
        }
        Ok(())
    }
}
