//! Reachable, stereo-only SE bank-0 processing. No ambience/reverb child,
//! custom bank, runtime player attachment, entitlement or full graph is implied.
use super::{
    Format, PlanarBlock, ffi,
    state::{Stage, status},
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StereoNoAmbienceControls {
    pub presence: f32,
    pub stereoizer: f32,
    #[serde(rename = "sshaper")]
    pub spectrum_shaper: bool,
}
impl Default for StereoNoAmbienceControls {
    fn default() -> Self {
        Self {
            presence: 0.0,
            stereoizer: 0.0,
            spectrum_shaper: true,
        }
    }
}
impl StereoNoAmbienceControls {
    pub fn validate(self) -> Result<()> {
        if !self.presence.is_finite()
            || !(-10.0..=10.0).contains(&self.presence)
            || !self.stereoizer.is_finite()
            || !(0.0..=10.0).contains(&self.stereoizer)
        {
            return Err(Error::InvalidInput(
                "unsupported no-ambience stereo controls",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StereoNoAmbienceSnapshot {
    pub side_gain: f32,
    pub applied_presence: f32,
    /// Each channel's five (envelope,power) pairs, not independent L/R gain.
    pub detector_history: [[f32; 10]; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StereoNoAmbienceResult {
    pub valid_frames: usize,
    pub end: bool,
}

pub struct StereoNoAmbienceStage {
    stage: Stage,
    failed: bool,
}
impl StereoNoAmbienceStage {
    pub fn new(format: Format) -> Result<Self> {
        // Reject unsupported shapes before allocating the native child state.
        // Original SIMD rounded stores are safe within these channel strides.
        if format.channels() != 2
            || format.capacity() < 4
            || !format.capacity().is_multiple_of(4)
            || format.rate().fract() != 0.0
        {
            return Err(Error::InvalidInput(
                "no-ambience stereo requires integer rate and aligned stereo capacity",
            ));
        }
        let mut stage = Stage::new(7, format)?;
        // SAFETY: owned aligned kind-7 allocation and validated fixed format.
        status(unsafe {
            ffi::cumulus_stereo_zero_init(
                stage.state.pointer(),
                format.rate(),
                format.capacity() as u32,
            )
        })?;
        Ok(Self {
            stage,
            failed: false,
        })
    }
    pub fn format(&self) -> Format {
        self.stage.format
    }
    fn available(&self) -> Result<()> {
        if self.failed {
            Err(Error::State(
                "no-ambience stereo failed; recreate the stage",
            ))
        } else {
            Ok(())
        }
    }
    pub fn set_controls(&mut self, controls: StereoNoAmbienceControls) -> Result<()> {
        self.available()?;
        controls.validate()?;
        // SAFETY: initialized exclusive state; C validates the entire tuple
        // before changing any control or presence-dirty bit.
        status(unsafe {
            ffi::netease_stereo_zero_parameters(
                self.stage.state.pointer(),
                controls.presence,
                controls.stereoizer,
                controls.spectrum_shaper.into(),
            )
        })
    }
    pub fn set_bypass(&mut self, bypass: bool) -> Result<()> {
        self.available()?;
        // SAFETY: owned initialized state; enable edge resets only child history.
        status(unsafe {
            ffi::netease_stereo_zero_bypass(self.stage.state.pointer(), bypass.into())
        })
    }
    pub fn reset(&mut self) -> Result<()> {
        self.available()?;
        // SAFETY: exclusive initialized state. Parent gain history is preserved.
        status(unsafe { ffi::netease_stereo_zero_reset(self.stage.state.pointer()) })
    }
    pub fn snapshot(&self) -> Result<StereoNoAmbienceSnapshot> {
        let mut values = [0.0; 22];
        // SAFETY: read-only initialized state and exactly 22 writable floats.
        status(unsafe {
            ffi::netease_stereo_zero_history(self.stage.state.const_pointer(), values.as_mut_ptr())
        })?;
        if values.iter().any(|v| !v.is_finite()) {
            return Err(Error::Media("non-finite stereo detector state"));
        }
        Ok(StereoNoAmbienceSnapshot {
            side_gain: values[0],
            applied_presence: values[1],
            detector_history: [
                values[2..12].try_into().expect("ten values"),
                values[12..22].try_into().expect("ten values"),
            ],
        })
    }
    /// Process in place. Planar stride remains full capacity; only returned
    /// valid_frames may be consumed. Bypass fades touch the full padded block.
    /// This branch preserves frames/EOF and has no separate reverb tail drain.
    pub fn process(
        &mut self,
        block: &mut PlanarBlock,
        frames: usize,
        end: bool,
    ) -> Result<StereoNoAmbienceResult> {
        self.available()?;
        if block.format() != self.stage.format
            || frames == 0
            || frames > self.stage.format.capacity()
            || block.samples().iter().any(|v| !v.is_finite())
        {
            return Err(Error::InvalidInput("invalid no-ambience stereo block"));
        }
        let result = self
            .stage
            .process(block, frames, ffi::netease_stereo_zero_process)
            .and_then(|()| self.snapshot().map(|_| ()));
        if let Err(error) = result {
            block.samples_mut().fill(0.0);
            self.failed = true;
            return Err(error);
        }
        Ok(StereoNoAmbienceResult {
            valid_frames: frames,
            end,
        })
    }
}
