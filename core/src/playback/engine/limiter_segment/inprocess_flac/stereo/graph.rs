//! Single held-source graph lifetime. Preparing a decoder is not PCM use.
#[cfg(test)]
use crate::effects::{
    dsp::LimiterStage,
    material::{ModernLimiterPlan, ModernLimiterStages},
};
use crate::{
    Error, Result,
    effects::{
        dsp::{AmbienceOneResult, Format, PlanarBlock},
        stereo::{
            LocalEqualizerMaterialPlan, LocalEqualizerStereoMaterial, LocalFixedStereoMaterial,
            LocalStereoMaterialPlan,
        },
    },
    playback::engine::stereo_pcm::StereoGraphControl,
};

pub(super) struct SourceGraph {
    material: Material,
    driven: bool,
}
enum Material {
    Cold(LocalFixedStereoMaterial),
    Local(LocalEqualizerStereoMaterial),
    #[cfg(test)]
    Modern(Box<ModernBlocks>),
}
impl SourceGraph {
    pub fn new(format: Format, plan: LocalStereoMaterialPlan) -> Result<Self> {
        Ok(Self {
            material: Material::Cold(LocalFixedStereoMaterial::new(format, plan)?),
            driven: false,
        })
    }
    pub fn new_local(format: Format, plan: LocalEqualizerMaterialPlan) -> Result<Self> {
        Ok(Self {
            material: Material::Local(LocalEqualizerStereoMaterial::new(format, plan)?),
            driven: false,
        })
    }
    #[cfg(test)]
    pub fn new_modern_test(format: Format, plan: &ModernLimiterPlan) -> Result<Self> {
        let (stages, limiter) = plan.native_test_blocks(format)?;
        Ok(Self {
            material: Material::Modern(Box::new(ModernBlocks {
                format,
                stages,
                limiter,
                failed: false,
            })),
            driven: false,
        })
    }
    pub fn format(&self) -> Format {
        match &self.material {
            Material::Cold(graph) => graph.format(),
            Material::Local(graph) => graph.format(),
            #[cfg(test)]
            Material::Modern(graph) => graph.format,
        }
    }
    /// Fresh zero-start preserves the graph's initial transition: cold keeps
    /// its original off edges, ordinary Init has already reset then applied
    /// the selected material. A real seek resets without reapplying settings.
    pub fn prepare(&mut self, first: u64) -> Result<()> {
        if self.driven || first != 0 {
            match &mut self.material {
                Material::Cold(graph) => graph.reset()?,
                Material::Local(graph) => graph.reset()?,
                #[cfg(test)]
                Material::Modern(graph) => graph.reset()?,
            }
            self.driven = true;
        }
        Ok(())
    }
    #[cfg(test)]
    pub fn configure(&mut self, plan: LocalStereoMaterialPlan) -> Result<()> {
        self.apply(StereoGraphControl::Cold(plan))
    }
    pub fn apply(&mut self, control: StereoGraphControl) -> Result<()> {
        match (&mut self.material, control) {
            (Material::Cold(graph), StereoGraphControl::Cold(plan)) => graph.configure(plan),
            (Material::Local(graph), StereoGraphControl::ApplyLocal(plan)) => {
                graph.apply_material(plan)
            }
            (Material::Local(graph), StereoGraphControl::DisableLocalMaster) => {
                graph.disable_master()
            }
            _ => Err(Error::InvalidInput(
                "control belongs to another stereo graph",
            )),
        }
    }
    pub fn process(
        &mut self,
        pcm: &mut PlanarBlock,
        frames: usize,
        end: bool,
    ) -> Result<AmbienceOneResult> {
        // A failed invocation cannot be treated as an unused graph either;
        // the underlying material's numerical quarantine remains authoritative.
        self.driven = true;
        match &mut self.material {
            Material::Cold(graph) => graph.process(pcm, frames, end),
            Material::Local(graph) => graph.process(pcm, frames, end),
            #[cfg(test)]
            Material::Modern(graph) => graph.process(pcm, frames, end),
        }
    }
}

#[cfg(test)]
struct ModernBlocks {
    format: Format,
    stages: ModernLimiterStages,
    limiter: LimiterStage,
    failed: bool,
}

#[cfg(test)]
impl ModernBlocks {
    fn reset(&mut self) -> Result<()> {
        if self.failed {
            return Err(Error::State("held modern limiter graph is quarantined"));
        }
        if let Err(error) = self.stages.reset() {
            self.failed = true;
            return Err(error);
        }
        self.limiter.reset_stream_state();
        Ok(())
    }

    fn process(
        &mut self,
        pcm: &mut PlanarBlock,
        frames: usize,
        end: bool,
    ) -> Result<AmbienceOneResult> {
        if self.failed {
            return Err(Error::State("held modern limiter graph is quarantined"));
        }
        let frames = u32::try_from(frames)
            .map_err(|_| Error::InvalidInput("invalid held modern limiter frame count"))?;
        match self.stages.process(&mut self.limiter, pcm, frames, end) {
            Ok(flow) => Ok(AmbienceOneResult {
                valid_frames: flow.output_frames as usize,
                end: flow.output_end,
            }),
            Err(error) => {
                self.failed = true;
                Err(error)
            }
        }
    }
}

#[cfg(all(
    test,
    any(target_os = "linux", target_os = "windows"),
    target_arch = "x86_64"
))]
#[path = "graph_tests.rs"]
mod tests;
