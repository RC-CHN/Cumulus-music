use super::{
    Format, PlanarBlock, ffi,
    state::{Stage, status},
};
use crate::Result;

pub struct GraphicEq(Stage);

impl GraphicEq {
    pub fn new(format: Format) -> Result<Self> {
        let mut stage = Stage::new(1, format)?;
        // SAFETY: matching owned allocation; validated scalar format.
        status(unsafe {
            ffi::netease_geq_stage_init(
                stage.state.pointer(),
                format.rate(),
                format.channels() as u32,
                format.capacity() as u32,
            )
        })?;
        Ok(Self(stage))
    }
    pub fn set_gains(&mut self, gains: &[f32; 10]) -> Result<()> {
        // SAFETY: initialized state, ten readable floats; C validates atomically.
        status(unsafe { ffi::netease_geq_stage_set(self.0.state.pointer(), gains.as_ptr()) })
    }
    pub fn set_bypass(&mut self, bypass: bool) -> Result<()> {
        // SAFETY: initialized state and a valid boolean.
        status(unsafe { ffi::netease_geq_stage_bypass(self.0.state.pointer(), bypass.into()) })
    }
    pub fn process(&mut self, block: &mut PlanarBlock, frames: usize) -> Result<()> {
        self.0
            .process(block, frames, ffi::netease_geq_stage_process)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BassTrebleSettings {
    pub bass_db: f32,
    pub treble_db: f32,
    pub low_frequency: f32,
    pub low_q: f32,
    pub high_frequency: f32,
    pub high_q: f32,
}

impl Default for BassTrebleSettings {
    fn default() -> Self {
        Self {
            bass_db: 0.0,
            treble_db: 0.0,
            low_frequency: 120.0,
            low_q: 5.0,
            high_frequency: 5000.0,
            high_q: 5.0,
        }
    }
}

pub struct BassTreble(Stage);

impl BassTreble {
    pub fn new(format: Format) -> Result<Self> {
        let mut stage = Stage::new(2, format)?;
        // SAFETY: matching allocation and validated scalar format.
        status(unsafe {
            ffi::netease_bt_init(
                stage.state.pointer(),
                format.rate(),
                format.channels() as u32,
                format.capacity() as u32,
            )
        })?;
        Ok(Self(stage))
    }
    pub fn configure(&mut self, values: BassTrebleSettings) -> Result<()> {
        // SAFETY: initialized state; C validates all fields before committing.
        status(unsafe {
            ffi::netease_bt_set(
                self.0.state.pointer(),
                values.bass_db,
                values.treble_db,
                values.low_frequency,
                values.low_q,
                values.high_frequency,
                values.high_q,
            )
        })
    }
    pub fn set_bypass(&mut self, bypass: bool) -> Result<()> {
        // SAFETY: initialized state and valid boolean.
        status(unsafe { ffi::netease_bt_bypass(self.0.state.pointer(), bypass.into()) })
    }
    pub fn process(&mut self, block: &mut PlanarBlock, frames: usize) -> Result<()> {
        self.0.process(block, frames, ffi::netease_bt_process)
    }
}
