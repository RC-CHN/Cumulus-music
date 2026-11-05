//! Executable fixed local-material scope compared with the complete original
//! SDK graph. This is not support for arbitrary SDK presets or RVB-on.
use super::LocalStereoMaterialPlan;
use crate::{
    Error, Result,
    effects::{
        dsp::{
            AmbienceOneResult, Format, LimiterSettings, LimiterStage, LocalFixedStage, PlanarBlock,
        },
        material::MATERIAL_BEGIN_LIMITER,
    },
};

/// Explicit research/native caller only; no automatic player/UI activation.
/// Requires the strict free local JSON scope and independently proven format.
pub struct LocalFixedStereoMaterial {
    stage: LocalFixedStage,
    limiter: LimiterStage,
    format: Format,
    failed: bool,
}
impl LocalFixedStereoMaterial {
    pub fn new(format: Format, plan: LocalStereoMaterialPlan) -> Result<Self> {
        let stage = LocalFixedStage::new(format, plan.stereo())?;
        let p = MATERIAL_BEGIN_LIMITER;
        let limiter = LimiterStage::new(
            format,
            LimiterSettings {
                enabled: true,
                threshold_db: p.threshold,
                attack_seconds: p.attack,
                release_seconds: p.release,
                input_gain_db: 0.0,
                output_gain_db: p.outputgain,
                infinite_ratio: p.infinite_ratio,
                ratio: p.ratio,
            },
        )?;
        Ok(Self {
            stage,
            limiter,
            format,
            failed: false,
        })
    }
    pub fn format(&self) -> Format {
        self.format
    }
    pub fn lookahead_frames(&self) -> u32 {
        self.limiter.lookahead_frames()
    }
    fn available(&self) -> Result<()> {
        if self.failed {
            Err(Error::State("fixed local stereo material failed"))
        } else {
            Ok(())
        }
    }
    /// On=false follows EFF390: preserve applied controls/bank, change bypass.
    pub fn configure(&mut self, plan: LocalStereoMaterialPlan) -> Result<()> {
        self.available()?;
        self.stage.configure(plan.stereo())
    }
    /// Original 1B960 node reset, including wrapper previous=current and
    /// final B0 lookahead discard. Does not recreate any state or consume PCM.
    pub fn reset(&mut self) -> Result<()> {
        self.available()?;
        self.stage.reset()?;
        self.limiter.reset_stream_state();
        Ok(())
    }
    pub fn process(
        &mut self,
        block: &mut PlanarBlock,
        frames: usize,
        end: bool,
    ) -> Result<AmbienceOneResult> {
        self.available()?;
        if block.format() != self.format
            || frames > self.format.capacity()
            || block.samples().iter().any(|v| !v.is_finite())
        {
            return Err(Error::InvalidInput(
                "invalid fixed local stereo material PCM",
            ));
        }
        let result = self.stage.process(block, frames, end).and_then(|flow| {
            self.limiter
                .process(block, flow.valid_frames as u32, flow.end)
        });
        match result {
            Ok(flow) if block.samples().iter().all(|v| v.is_finite()) => Ok(AmbienceOneResult {
                valid_frames: flow.output_frames as usize,
                end: flow.output_end,
            }),
            _ => {
                block.samples_mut().fill(0.0);
                self.failed = true;
                Err(Error::Media(
                    "fixed local stereo material processing failed",
                ))
            }
        }
    }
}
