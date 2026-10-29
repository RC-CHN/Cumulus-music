use crate::control::Settings;
use cumulus_core::{
    Error, Result,
    effects::dsp::{Format, GraphicEq, LoudnessParameters, LoudnessStage, PlanarBlock},
};

pub(crate) enum Stage {
    Equalizer(GraphicEq),
    Loudness(LoudnessStage),
}
impl Stage {
    pub fn new(format: Format, loudness: bool) -> Result<Self> {
        if loudness {
            Ok(Self::Loudness(LoudnessStage::new(format)))
        } else {
            Ok(Self::Equalizer(GraphicEq::new(format)?))
        }
    }
    pub fn recreate(&self, format: Format) -> Result<Self> {
        Self::new(format, matches!(self, Self::Loudness(_)))
    }
    pub fn configure(&mut self, settings: Settings) -> Result<()> {
        match (self, settings) {
            (Self::Equalizer(stage), Settings::Equalizer(settings)) => {
                stage.set_gains(settings.gains())?;
                stage.set_bypass(!settings.enabled())
            }
            (Self::Loudness(stage), Settings::Loudness(settings)) => {
                // M116 toggles first and only sends new gain while enabled.
                stage.set_bypass(!settings.enabled())?;
                if settings.enabled() {
                    stage.set_parameters(LoudnessParameters {
                        gain_db: settings.gain_db(),
                        peak: 0.0,
                    })?;
                }
                Ok(())
            }
            _ => Err(Error::State("filter configuration kind mismatch")),
        }
    }
    pub fn process(&mut self, block: &mut PlanarBlock, frames: usize) -> Result<()> {
        match self {
            Self::Equalizer(stage) => stage.process(block, frames),
            Self::Loudness(stage) => stage.process(block, frames),
        }
    }
}
