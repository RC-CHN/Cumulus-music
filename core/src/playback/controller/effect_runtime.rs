use super::state::Actor;
use crate::{
    Error, Result,
    effects::{equalizer::EqualizerStatus, loudness::LoudnessStatus},
    playback::engine::EngineSnapshot,
};

#[derive(Clone, Copy)]
enum ProcessorFault {
    Equalizer = 1,
    Loudness = 2,
    Parametric = 4,
}

impl ProcessorFault {
    fn classify(error: &Error) -> Option<Self> {
        match error.root() {
            Error::EqualizerProcessorFailed => Some(Self::Equalizer),
            Error::LoudnessProcessorFailed => Some(Self::Loudness),
            Error::ParametricProcessorFailed => Some(Self::Parametric),
            _ => None,
        }
    }
}

pub(super) fn is_processor_fault(error: &Error) -> bool {
    ProcessorFault::classify(error).is_some()
}

impl Actor {
    pub(super) async fn poll_with_effect_recovery(&mut self) -> Result<EngineSnapshot> {
        let result = super::engine(self.engine.clone(), self.guard(), |engine| engine.poll()).await;
        self.recover_effect_poll(result).await
    }

    pub(super) async fn recover_effect_poll(
        &mut self,
        mut result: Result<EngineSnapshot>,
    ) -> Result<EngineSnapshot> {
        let mut detached = 0;
        while let Some(fault) = result.as_ref().err().and_then(ProcessorFault::classify) {
            // Every distinct failing processor gets at most one removal and
            // resample. Another report for a removed node cannot be ignored.
            let bit = fault as u8;
            if detached & bit != 0 {
                break;
            }
            match fault {
                ProcessorFault::Equalizer => {
                    self.equalizer.status = EqualizerStatus::Failed;
                    super::engine(self.engine.clone(), self.guard(), |engine| {
                        engine.clear_equalizer()
                    })
                    .await?;
                }
                ProcessorFault::Loudness => {
                    self.loudness.status = LoudnessStatus::Failed;
                    super::engine(self.engine.clone(), self.guard(), |engine| {
                        engine.clear_loudness()
                    })
                    .await?;
                }
                ProcessorFault::Parametric => self.detach_failed_parametric().await?,
            }
            detached |= bit;
            result = super::engine(self.engine.clone(), self.guard(), |engine| engine.poll()).await;
        }
        result
    }
}
