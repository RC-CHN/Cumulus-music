use super::{
    Format, PlanarBlock, ffi,
    state::{State, status},
};
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Immutable response data owned for as long as any native rotation state uses
/// it. Layout is ear[2]/angle[37]/complex frequency[513]/real-imaginary[2].
#[derive(Clone)]
pub struct RotationResponses {
    values: Arc<[f32]>,
    sha256: String,
}

impl RotationResponses {
    pub const BYTE_LENGTH: usize = 2 * 37 * 513 * 2 * 4;

    pub fn from_le_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != Self::BYTE_LENGTH {
            return Err(Error::Media("invalid rotation response length"));
        }
        let values: Vec<_> = bytes
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().expect("four bytes")))
            .collect();
        if values.iter().any(|value| !value.is_finite()) {
            return Err(Error::Media("non-finite rotation response"));
        }
        Ok(Self {
            values: values.into(),
            sha256: hex::encode(Sha256::digest(bytes)),
        })
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

fn period_result(code: i32) -> Result<bool> {
    match code {
        0 => Ok(true),
        1 => Ok(false),
        _ => Err(Error::InvalidInput("invalid rotation period")),
    }
}

fn check_block(format: Format, block: &PlanarBlock, frames: usize, failed: bool) -> Result<()> {
    if failed {
        return Err(Error::State("rotation failed; recreate the stage"));
    }
    if format != block.format() || frames > format.capacity() {
        return Err(Error::InvalidInput(
            "rotation block format or frame count mismatch",
        ));
    }
    if block.samples().iter().any(|value| !value.is_finite()) {
        return Err(Error::InvalidInput("non-finite rotation input"));
    }
    Ok(())
}

fn check_output(block: &mut PlanarBlock, frames: u32, failed: &mut bool) -> Result<usize> {
    if frames as usize > block.format().capacity()
        || block.samples().iter().any(|value| !value.is_finite())
    {
        block.samples_mut().fill(0.0);
        *failed = true;
        return Err(Error::Media("invalid rotation output"));
    }
    Ok(frames as usize)
}

/// Native 44.1kHz stereo, 256-frame convolution core. The first processed block
/// returns zero valid frames while still advancing phase and overlap history.
pub struct RotationKernel {
    state: State,
    _responses: RotationResponses,
    format: Format,
    failed: bool,
}

impl RotationKernel {
    pub fn new(responses: RotationResponses, phase: f32) -> Result<Self> {
        let mut state = State::new(4)?;
        // SAFETY: matching layout, immutable response allocation retained below;
        // C validates exact count and phase before changing state.
        status(unsafe {
            ffi::netease_rotate_init(
                state.pointer(),
                responses.values.as_ptr(),
                responses.values.len(),
                phase,
            )
        })?;
        Ok(Self {
            state,
            _responses: responses,
            format: Format::new(44100.0, 2, 256)?,
            failed: false,
        })
    }

    /// Returns false when the official setter ignores a finite period <1s.
    pub fn set_period(&mut self, seconds: f32) -> Result<bool> {
        // SAFETY: initialized owned state; C checks non-finite input.
        period_result(unsafe { ffi::netease_rotate_period(self.state.pointer(), seconds) })
    }

    /// Clear overlap/warm-up state; preserve phase as the native reset does.
    pub fn reset(&mut self) {
        // SAFETY: initialized exclusive state, immutable table stays alive.
        unsafe { ffi::netease_rotate_reset(self.state.pointer()) };
        // A numerical failure is deliberately not cleared by this partial reset.
    }

    pub fn phase(&self) -> f32 {
        // SAFETY: read-only field accessor of initialized live state.
        unsafe { ffi::netease_rotate_phase(self.state.const_pointer()) }
    }

    pub fn interpolated_responses(&self) -> &[f32] {
        // SAFETY: the C accessor returns its inline response[2][1026]. This
        // slice borrows self, excluding mutation and preventing it outliving C.
        unsafe {
            std::slice::from_raw_parts(
                ffi::netease_rotate_response(self.state.const_pointer()),
                2052,
            )
        }
    }

    pub fn process(&mut self, block: &mut PlanarBlock, frames: usize) -> Result<usize> {
        check_block(self.format, block, frames, self.failed)?;
        let mut valid = frames as u32;
        // SAFETY: full stereo 256-frame storage, valid<=256, disjoint state;
        // native response pointer remains backed by _responses for this call.
        status(unsafe {
            ffi::netease_rotate_process(
                self.state.pointer(),
                block.samples_mut().as_mut_ptr(),
                &mut valid,
            )
        })?;
        check_output(block, valid, &mut self.failed)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RotationResult {
    pub valid_frames: usize,
    pub end: bool,
    pub phase: f32,
}

/// Recovered 44.1kHz stereo FIFO wrapper. Other sample rates require a separate
/// rate-conversion stage; they are explicitly rejected here.
pub struct RotationStage {
    state: State,
    _responses: RotationResponses,
    format: Format,
    failed: bool,
}

impl RotationStage {
    pub fn new(format: Format, responses: RotationResponses) -> Result<Self> {
        if format.rate() != 44100.0 || format.channels() != 2 || format.capacity() < 256 {
            return Err(Error::InvalidInput(
                "rotation requires 44.1kHz stereo and capacity >=256",
            ));
        }
        let mut state = State::new(5)?;
        // SAFETY: matching native allocation and bounded capacity; response
        // allocation is immutable and retained for this state's full lifetime.
        status(unsafe {
            ffi::netease_rotate_stage_init(
                state.pointer(),
                responses.values.as_ptr(),
                responses.values.len(),
                format.capacity() as u32,
            )
        })?;
        Ok(Self {
            state,
            _responses: responses,
            format,
            failed: false,
        })
    }

    pub fn set_period(&mut self, seconds: f32) -> Result<bool> {
        // SAFETY: initialized exclusive state and checked native setter.
        period_result(unsafe { ffi::netease_rotate_stage_period(self.state.pointer(), seconds) })
    }
    pub fn set_bypass(&mut self, bypass: bool) -> Result<()> {
        // SAFETY: initialized exclusive state and valid boolean.
        status(unsafe { ffi::netease_rotate_stage_bypass(self.state.pointer(), bypass.into()) })
    }
    pub fn reset(&mut self) {
        // SAFETY: clears owned FIFO and overlap without releasing response data.
        unsafe { ffi::netease_rotate_stage_reset(self.state.pointer()) };
    }
    pub fn phase(&self) -> f32 {
        // SAFETY: read-only field accessor of initialized state.
        unsafe { ffi::netease_rotate_stage_phase(self.state.const_pointer()) }
    }
    pub fn process(
        &mut self,
        block: &mut PlanarBlock,
        frames: usize,
        end: bool,
    ) -> Result<RotationResult> {
        check_block(self.format, block, frames, self.failed)?;
        let (mut valid, mut finished) = (frames as u32, u32::from(end));
        // SAFETY: exact full-capacity planar allocation, in/out integers are
        // distinct valid pointers, native FIFO bounds checked before mutation.
        status(unsafe {
            ffi::netease_rotate_stage_process(
                self.state.pointer(),
                block.samples_mut().as_mut_ptr(),
                &mut valid,
                &mut finished,
            )
        })?;
        let valid_frames = check_output(block, valid, &mut self.failed)?;
        Ok(RotationResult {
            valid_frames,
            end: finished != 0,
            phase: self.phase(),
        })
    }
}
