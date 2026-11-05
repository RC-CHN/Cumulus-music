//! Fixed-bank1 SE component, including its real environment child.
//! This is not the complete GEQ/BT/RVB/material graph or a live engine.
use super::{
    AmbienceOneResult, AmbienceOneSnapshot, Format, PlanarBlock, StereoNoAmbienceControls,
    ambience_one::allocate_storage,
    ffi,
    state::{State, status},
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StereoAmbienceOneControls {
    pub presence: f32,
    pub stereoizer: f32,
    #[serde(rename = "sshaper")]
    pub spectrum_shaper: bool,
}
impl Default for StereoAmbienceOneControls {
    fn default() -> Self {
        Self {
            presence: 0.0,
            stereoizer: 0.0,
            spectrum_shaper: true,
        }
    }
}
impl StereoAmbienceOneControls {
    pub fn validate(self) -> Result<()> {
        // Sharing setter ranges does not select bank0 processing.
        StereoNoAmbienceControls {
            presence: self.presence,
            stereoizer: self.stereoizer,
            spectrum_shaper: self.spectrum_shaper,
        }
        .validate()
    }
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StereoAmbienceOneSnapshot {
    pub side_gain: f32,
    pub applied_presence: f32,
    pub detector_history: [[f32; 10]; 2],
    pub environment: AmbienceOneSnapshot,
}

pub struct StereoAmbienceOneStage {
    state: State,
    format: Format,
    _storage: Box<[f32]>,
    dry: PlanarBlock,
    failed: bool,
}
impl StereoAmbienceOneStage {
    pub fn new(format: Format) -> Result<Self> {
        if format.channels() != 2
            || format.rate().fract() != 0.0
            || format.capacity() < 4
            || !format.capacity().is_multiple_of(4)
        {
            return Err(Error::InvalidInput(
                "bank1 SE requires integer-rate aligned stereo",
            ));
        }
        let rate = format.rate() as u32;
        let mut storage = allocate_storage(rate)?;
        let mut state = State::new(9)?;
        // SAFETY: queried C layout, exclusive zeroed object and exact stable
        // owned arena, retained for the object's complete lifetime.
        status(unsafe {
            ffi::cumulus_stereo_one_init(
                state.pointer(),
                rate,
                format.capacity() as u32,
                storage.as_mut_ptr(),
                storage.len(),
            )
        })?;
        Ok(Self {
            state,
            format,
            _storage: storage,
            dry: PlanarBlock::new(format),
            failed: false,
        })
    }
    pub fn format(&self) -> Format {
        self.format
    }
    fn available(&self) -> Result<()> {
        if self.failed {
            Err(Error::State("bank1 SE failed; recreate the stage"))
        } else {
            Ok(())
        }
    }
    pub fn set_controls(&mut self, controls: StereoAmbienceOneControls) -> Result<()> {
        self.available()?;
        controls.validate()?;
        // SAFETY: exclusive initialized state and atomically validated tuple.
        status(unsafe {
            ffi::netease_stereo_one_parameters(
                self.state.pointer(),
                controls.presence,
                controls.stereoizer,
                controls.spectrum_shaper.into(),
            )
        })
    }
    pub fn set_bypass(&mut self, bypass: bool) -> Result<()> {
        self.available()?;
        // SAFETY: exclusive initialized state, original reset on enable.
        status(unsafe { ffi::netease_stereo_one_bypass(self.state.pointer(), bypass.into()) })
    }
    /// Preserves parent gains, ring cursors and any current EOF budget.
    pub fn reset(&mut self) -> Result<()> {
        self.available()?;
        // SAFETY: exclusive initialized object and still-owned full arena.
        status(unsafe { ffi::netease_stereo_one_reset(self.state.pointer()) })
    }
    pub fn snapshot(&self) -> Result<StereoAmbienceOneSnapshot> {
        let mut parent = [0.0; 22];
        let mut indices = [0; 87];
        let mut history = [0.0; 70];
        // SAFETY: read-only initialized state and exact fixed output arrays.
        status(unsafe {
            ffi::netease_stereo_one_history(self.state.const_pointer(), parent.as_mut_ptr())
        })?;
        // SAFETY: same lifetime/layout, separate exact environment arrays.
        status(unsafe {
            ffi::netease_stereo_one_environment(
                self.state.const_pointer(),
                indices.as_mut_ptr(),
                history.as_mut_ptr(),
            )
        })?;
        if parent.iter().any(|x| !x.is_finite()) {
            return Err(Error::Media("non-finite bank1 SE history"));
        }
        Ok(StereoAmbienceOneSnapshot {
            side_gain: parent[0],
            applied_presence: parent[1],
            detector_history: [
                parent[2..12].try_into().expect("ten values"),
                parent[12..22].try_into().expect("ten values"),
            ],
            environment: AmbienceOneSnapshot::from_arrays(indices, history)?,
        })
    }
    /// Zero valid frames still retain complete initialized PCM and band caches.
    /// The original shaper advances once on their retained first samples;
    /// this does not fabricate an audio frame. Tail may increase valid frames.
    pub fn process(
        &mut self,
        block: &mut PlanarBlock,
        frames: usize,
        end: bool,
    ) -> Result<AmbienceOneResult> {
        self.available()?;
        if block.format() != self.format
            || frames > self.format.capacity()
            || block.samples().iter().any(|x| !x.is_finite())
        {
            return Err(Error::InvalidInput("invalid bank1 SE planar block"));
        }
        let mut valid = frames as u32;
        let mut ended = u32::from(end);
        // SAFETY: full initialized exclusive PCM and disjoint owned dry/arena;
        // zero valid frames keeps all native storage allocated and alive.
        let result = status(unsafe {
            ffi::netease_stereo_one_process(
                self.state.pointer(),
                block.samples_mut().as_mut_ptr(),
                &mut valid,
                &mut ended,
                self.dry.samples_mut().as_mut_ptr(),
            )
        });
        if result.is_err()
            || valid as usize > self.format.capacity()
            || ended > 1
            || block.samples().iter().any(|x| !x.is_finite())
            || self.snapshot().is_err()
        {
            block.samples_mut().fill(0.0);
            self.failed = true;
            return Err(Error::Media("bank1 SE processing failed"));
        }
        Ok(AmbienceOneResult {
            valid_frames: valid as usize,
            end: ended != 0,
        })
    }
}
