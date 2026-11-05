//! Native fixed-bank1 SE host. This component is not a whole local material.
pub mod abi;
mod host;
mod material;
mod stream;
use super::dsp::StereoAmbienceOneControls;
use crate::Result;
pub use host::{StereoHost, StereoInstance};
pub use material::{LocalStereoMaterialPlan, LocalStereoSubgraph};
pub use stream::LocalStereoStream;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StereoSettings {
    enabled: bool,
    controls: StereoAmbienceOneControls,
}
impl StereoSettings {
    pub fn new(enabled: bool, presence: f32, stereoizer: f32, shaper: bool) -> Result<Self> {
        let controls = StereoAmbienceOneControls {
            presence,
            stereoizer,
            spectrum_shaper: shaper,
        };
        controls.validate()?;
        Ok(Self { enabled, controls })
    }
    pub fn enabled(self) -> bool {
        self.enabled
    }
    pub fn controls(self) -> StereoAmbienceOneControls {
        self.controls
    }
}
