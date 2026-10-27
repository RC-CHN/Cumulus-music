use super::{
    Format, ParametricKind, PlanarBlock, ffi,
    state::{Stage, status},
};
use crate::{Error, Result};

#[derive(Clone, Copy, Debug)]
pub struct ParametricBand {
    pub kind: ParametricKind,
    pub gain_db: f32,
    pub frequency: f32,
    pub q: f32,
    pub enabled: bool,
}

pub struct ParametricEq(Stage);

impl ParametricEq {
    pub fn new(format: Format, has_lfe: bool, process_lfe: bool) -> Result<Self> {
        let mut stage = Stage::new(3, format)?;
        // SAFETY: matching allocation, bounded format and boolean flags.
        status(unsafe {
            ffi::netease_peq_init(
                stage.state.pointer(),
                format.rate(),
                format.channels() as u32,
                format.capacity() as u32,
                has_lfe.into(),
                process_lfe.into(),
            )
        })?;
        Ok(Self(stage))
    }
    pub fn format(&self) -> Format {
        self.0.format
    }
    pub fn set_band(&mut self, index: usize, band: ParametricBand) -> Result<()> {
        if index >= 9 {
            return Err(Error::InvalidInput("PEQ band index out of bounds"));
        }
        // SAFETY: index checked, typed enum, C validates before state mutation.
        status(unsafe {
            ffi::netease_peq_band(
                self.0.state.pointer(),
                index as u32,
                band.kind as u32,
                band.gain_db,
                band.frequency,
                band.q,
                band.enabled.into(),
            )
        })
    }
    pub fn set_gain(&mut self, gain_db: f32) -> Result<()> {
        // SAFETY: initialized state; C rejects non-finite gain before clamping.
        status(unsafe { ffi::netease_peq_set_gain(self.0.state.pointer(), gain_db) })
    }
    /// The original plugin fixes filter channel count at initialization;
    /// changing this flag later affects output gain routing only.
    pub fn set_process_lfe(&mut self, enabled: bool) -> Result<()> {
        // SAFETY: initialized state and a valid boolean.
        status(unsafe { ffi::netease_peq_set_lfe(self.0.state.pointer(), enabled.into()) })
    }
    pub fn set_bypass(&mut self, bypass: bool) -> Result<()> {
        // SAFETY: initialized state and a valid boolean.
        status(unsafe { ffi::netease_peq_bypass(self.0.state.pointer(), bypass.into()) })
    }
    pub fn process(&mut self, block: &mut PlanarBlock, frames: usize) -> Result<()> {
        self.0.process(block, frames, ffi::netease_peq_process)
    }
}
