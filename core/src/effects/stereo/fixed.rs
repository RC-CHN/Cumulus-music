//! Executable fixed local-material scope compared with the complete original
//! SDK graph. This is not support for arbitrary SDK presets or RVB-on.
use super::{LocalEqualizerMaterialPlan, LocalStereoMaterialPlan};
use crate::{
    Error, Result,
    effects::{
        dsp::{
            AmbienceOneResult, Format, LimiterSettings, LimiterStage, LocalFixedStage, PlanarBlock,
        },
        material::{MATERIAL_BEGIN_LIMITER, ModernLimiterPlan},
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

/// Original local equalizer in the verified stereo 44.1/48 kHz, capacity-2048
/// domain, with a bounded in-place transition to modern DSP. Source selection,
/// device, authorization, outer FIFO and effect activation belong to the caller.
pub struct LocalEqualizerStereoMaterial {
    graph: LocalFixedStereoMaterial,
    mode: LocalEqualizerMode,
    has_pcm: bool,
    ending: bool,
}

enum LocalEqualizerMode {
    Legacy { geq_only: bool },
    Modern(ModernLocalMaterial),
}

struct ModernLocalMaterial {
    plan: Box<ModernLimiterPlan>,
    active: bool,
}

impl ModernLocalMaterial {
    fn install(
        plan: &ModernLimiterPlan,
        stage: &mut LocalFixedStage,
        format: Format,
    ) -> Result<(Self, Option<LimiterStage>)> {
        let plan = Box::new(plan.clone());
        let limiter = plan.install_on_settled_local(stage, format)?;
        Ok((Self { plan, active: true }, limiter))
    }

    fn set_active(
        &mut self,
        stage: &mut LocalFixedStage,
        format: Format,
        active: bool,
    ) -> Result<Option<LimiterStage>> {
        if self.active == active {
            return Ok(None);
        }
        let limiter = self
            .plan
            .set_active_on_shared_local(stage, format, active)?;
        self.active = active;
        Ok(limiter)
    }
}
impl LocalEqualizerStereoMaterial {
    /// Reset the initialized nodes and apply the verified local material.
    pub fn new(format: Format, plan: LocalEqualizerMaterialPlan) -> Result<Self> {
        let graph =
            LocalFixedStereoMaterial::with_stage(format, LocalFixedStage::new_equalizer(format)?)?;
        let mut this = Self {
            graph,
            mode: LocalEqualizerMode::Legacy { geq_only: true },
            has_pcm: false,
            ending: false,
        };
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
        let geq_only = self.require_legacy()?;
        self.graph.available()?;
        self.graph
            .stage
            .apply_equalizer_material(plan.geq.as_ref(), plan.tone, plan.stereo)?;
        self.mode = LocalEqualizerMode::Legacy {
            geq_only: geq_only
                && plan.geq.is_some()
                && plan.tone == [0; 2]
                && plan.stereo == [0; 2],
        };
        Ok(())
    }
    /// Original master-off effect on this graph: fade GEQ/BT/SE to bypass
    /// without clearing their cached settings or re-priming default B0.
    pub fn disable_master(&mut self) -> Result<()> {
        self.require_legacy()?;
        self.graph.available()?;
        self.graph.stage.equalizer_master_off()?;
        self.mode = LocalEqualizerMode::Legacy { geq_only: false };
        Ok(())
    }

    fn require_legacy(&self) -> Result<bool> {
        match self.mode {
            LocalEqualizerMode::Legacy { geq_only } => Ok(geq_only),
            LocalEqualizerMode::Modern(_) => Err(Error::State(
                "old local controls cannot change a modern graph",
            )),
        }
    }

    pub(crate) fn initial_modern_ready(&self) -> bool {
        self.graph.available().is_ok()
            && self.has_pcm
            && !self.ending
            && matches!(self.mode, LocalEqualizerMode::Legacy { geq_only: true })
            && self.graph.stage.initial_modern_ready()
    }

    /// Apply a validated modern material to the currently driven GEQ-only
    /// object. Keeps the upstream wrappers and the caller's PCM/decoder
    /// state. It cannot select a source, grant playback rights, or install
    /// an effect in the application.
    pub fn apply_initial_modern(&mut self, plan: &ModernLimiterPlan) -> Result<()> {
        self.graph.available()?;
        if !self.initial_modern_ready() {
            return Err(Error::State(
                "initial modern effect requires a live local GEQ-only graph",
            ));
        }
        let format = self.graph.format();
        let (material, limiter) =
            ModernLocalMaterial::install(plan, &mut self.graph.stage, format)?;
        if let Some(limiter) = limiter {
            self.graph.limiter = limiter;
        }
        self.mode = LocalEqualizerMode::Modern(material);
        Ok(())
    }

    /// On the converted graph only, restore its original modern material or
    /// run master off. Retains decoder, PCM queues and wrapper previous flags.
    /// The caller's next quantum applies pending edges and any new B0 timing.
    pub fn set_modern_material_active(&mut self, active: bool) -> Result<()> {
        self.graph.available()?;
        let format = self.graph.format();
        let LocalEqualizerMode::Modern(material) = &mut self.mode else {
            return Err(Error::State("no modern material on the local graph"));
        };
        if !self.has_pcm || self.ending {
            return Err(Error::State("modern effect needs a live local graph"));
        }
        if let Some(limiter) = material.set_active(&mut self.graph.stage, format, active)? {
            self.graph.limiter = limiter;
        }
        Ok(())
    }
    /// Original 1B960 node reset, separate from a full apply or master off.
    /// Discards limiter lookahead and settles the wrapper previous flags.
    pub fn reset(&mut self) -> Result<()> {
        let result = self.graph.reset();
        if result.is_err() {
            self.graph.failed = true;
        } else {
            self.has_pcm = false;
            self.ending = false;
        }
        result
    }
    pub fn process(
        &mut self,
        block: &mut PlanarBlock,
        frames: usize,
        end: bool,
    ) -> Result<AmbienceOneResult> {
        let result = self.graph.process(block, frames, end);
        if result.is_ok() {
            self.has_pcm |= frames > 0;
            self.ending |= end;
        }
        result
    }
}

#[cfg(all(
    test,
    any(target_os = "linux", target_os = "windows"),
    target_arch = "x86_64"
))]
#[path = "initial_modern_tests.rs"]
mod initial_modern_tests;
