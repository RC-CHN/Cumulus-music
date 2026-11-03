use crate::control::Settings;
use cumulus_core::{
    Error, Result,
    effects::dsp::{
        BassTreble, BassTrebleSettings, Format, GraphicEq, LoudnessParameters, LoudnessStage,
        PlanarBlock,
    },
};

pub(crate) enum Stage {
    Equalizer {
        graphic: GraphicEq,
        tone: Option<BassTreble>,
        use_tone: bool,
    },
    Loudness(LoudnessStage),
}
impl Stage {
    pub fn new(format: Format, loudness: bool) -> Result<Self> {
        if loudness {
            Ok(Self::Loudness(LoudnessStage::new(format)))
        } else {
            Ok(Self::Equalizer {
                graphic: GraphicEq::new(format)?,
                tone: BassTreble::new(format).ok(),
                use_tone: false,
            })
        }
    }
    pub fn recreate(&self, format: Format) -> Result<Self> {
        Self::new(format, matches!(self, Self::Loudness(_)))
    }
    pub fn configure(&mut self, settings: Settings) -> Result<()> {
        match (self, settings) {
            (Self::Equalizer { graphic, .. }, Settings::Equalizer(settings)) => {
                graphic.set_gains(settings.gains())?;
                graphic.set_bypass(!settings.enabled())
            }
            (
                Self::Equalizer {
                    graphic,
                    tone,
                    use_tone,
                },
                Settings::EqualizerTone(settings),
            ) => {
                if settings.geq_active() {
                    graphic.set_gains(settings.gains())?;
                }
                graphic.set_bypass(!settings.geq_active())?;
                let Some(tone) = tone else {
                    return if settings.tone_active() {
                        Err(Error::State("tone DSP unavailable for this format"))
                    } else {
                        Ok(())
                    };
                };
                if settings.tone_active() {
                    let (bass, treble) = settings.tone();
                    tone.configure(BassTrebleSettings {
                        bass_db: bass as f32,
                        treble_db: treble as f32,
                        ..Default::default()
                    })?;
                }
                tone.set_bypass(!settings.tone_active())?;
                *use_tone = true;
                Ok(())
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
            Self::Equalizer {
                graphic,
                tone,
                use_tone,
            } => {
                graphic.process(block, frames)?;
                if *use_tone && let Some(tone) = tone {
                    tone.process(block, frames)?;
                }
                Ok(())
            }
            Self::Loudness(stage) => stage.process(block, frames),
        }
    }
}
