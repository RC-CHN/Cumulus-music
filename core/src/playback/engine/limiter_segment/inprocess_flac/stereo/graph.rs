//! Single held-source graph lifetime. Preparing a decoder is not PCM use.
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
    pub fn format(&self) -> Format {
        match &self.material {
            Material::Cold(graph) => graph.format(),
            Material::Local(graph) => graph.format(),
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
