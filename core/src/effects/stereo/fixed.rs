//! Executable fixed local-material scope compared with the complete original
//! SDK graph. This is not support for arbitrary SDK presets or RVB-on.
use super::{LocalEqualizerMaterialPlan, LocalStereoMaterialPlan};
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
        Self::with_stage(format, LocalFixedStage::new(format, plan.stereo())?)
    }
    fn with_stage(format: Format, stage: LocalFixedStage) -> Result<Self> {
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

/// Explicit old-schema master lifecycle in the witnessed stereo 44.1 kHz,
/// capacity-2048 scope. Device ownership, source rights, the outer FIFO and
/// ordinary player activation remain with the caller and are not provided here.
pub struct LocalFixedStereoMaster {
    graph: LocalFixedStereoMaterial,
}
impl LocalFixedStereoMaster {
    /// Outer Init's node reset, followed by a full fixed local material apply.
    pub fn new(format: Format, plan: LocalStereoMaterialPlan) -> Result<Self> {
        if !plan.stereo().enabled() {
            return Err(Error::InvalidInput(
                "fixed master scope requires enabled SE1",
            ));
        }
        let mut graph =
            LocalFixedStereoMaterial::with_stage(format, LocalFixedStage::new_master(format)?)?;
        graph.reset()?;
        graph.stage.apply_master_material(plan.stereo())?;
        Ok(Self { graph })
    }
    pub fn format(&self) -> Format {
        self.graph.format()
    }
    pub fn lookahead_frames(&self) -> u32 {
        self.graph.lookahead_frames()
    }
    /// Master off retains enabled/cached SE1, its state and the pending bypass
    /// edge. B0 is already permanently configured to the same native defaults;
    /// keep the existing PCM lookahead and gain envelope.
    pub fn disable_master(&mut self) -> Result<()> {
        self.graph.available()?;
        self.graph.stage.master_off()
    }
    /// Re-open via begin/defaults + the fixed component setters, not a raw
    /// master flag toggle. The final default B0 keeps its current stream state.
    pub fn apply_material(&mut self, plan: LocalStereoMaterialPlan) -> Result<()> {
        self.graph.available()?;
        self.graph.stage.apply_master_material(plan.stereo())
    }
    pub fn process(
        &mut self,
        block: &mut PlanarBlock,
        frames: usize,
        end: bool,
    ) -> Result<AmbienceOneResult> {
        self.graph.process(block, frames, end)
    }
}

/// Original old-schema local equalizer in the verified stereo 44.1/48 kHz,
/// capacity-2048 domain. No source selection, device, remote preset, outer
/// FIFO, desktop effect activation, or arbitrary SDK configuration is owned here.
pub struct LocalEqualizerStereoMaterial {
    graph: LocalFixedStereoMaterial,
}
impl LocalEqualizerStereoMaterial {
    /// Reset the initialized nodes and apply the verified local material.
    pub fn new(format: Format, plan: LocalEqualizerMaterialPlan) -> Result<Self> {
        let graph =
            LocalFixedStereoMaterial::with_stage(format, LocalFixedStage::new_equalizer(format)?)?;
        let mut this = Self { graph };
        this.reset()?;
        this.apply_material(plan)?;
        Ok(this)
    }
    pub fn format(&self) -> Format {
        self.graph.format()
    }
    pub fn lookahead_frames(&self) -> u32 {
        self.graph.lookahead_frames()
    }
    /// Original full material begin/defaults and setters on the same object.
    /// B0 retains the current lookahead and envelope.
    pub fn apply_material(&mut self, plan: LocalEqualizerMaterialPlan) -> Result<()> {
        self.graph.available()?;
        self.graph
            .stage
            .apply_equalizer_material(plan.geq.as_ref(), plan.tone, plan.stereo)
    }
    /// Original master-off effect on this graph: fade GEQ/BT/SE to bypass
    /// without clearing their cached settings or re-priming default B0.
    pub fn disable_master(&mut self) -> Result<()> {
        self.graph.available()?;
        self.graph.stage.equalizer_master_off()
    }
    /// Original 1B960 node reset, separate from a full apply or master off.
    /// Discards limiter lookahead and settles the wrapper previous flags.
    pub fn reset(&mut self) -> Result<()> {
        let result = self.graph.reset();
        if result.is_err() {
            self.graph.failed = true;
        }
        result
    }
    pub fn process(
        &mut self,
        block: &mut PlanarBlock,
        frames: usize,
        end: bool,
    ) -> Result<AmbienceOneResult> {
        self.graph.process(block, frames, end)
    }
}
