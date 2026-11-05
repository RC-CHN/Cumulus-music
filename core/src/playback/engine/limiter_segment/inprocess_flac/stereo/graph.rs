//! Single held-source graph lifetime. Preparing a decoder is not PCM use.
use crate::{
    Result,
    effects::{
        dsp::{AmbienceOneResult, Format, PlanarBlock},
        stereo::{LocalFixedStereoMaterial, LocalStereoMaterialPlan},
    },
};

pub(super) struct SourceGraph {
    material: LocalFixedStereoMaterial,
    driven: bool,
}
impl SourceGraph {
    pub fn new(format: Format, plan: LocalStereoMaterialPlan) -> Result<Self> {
        Ok(Self {
            material: LocalFixedStereoMaterial::new(format, plan)?,
            driven: false,
        })
    }
    pub fn format(&self) -> Format {
        self.material.format()
    }
    /// Fresh zero-start preserves every original cold off transition. A real
    /// logical seek, including rewinding an already used source, calls1B960.
    /// Failed/cancelled decoder setup before any PCM does not consume fresh.
    pub fn prepare(&mut self, first: u64) -> Result<()> {
        if self.driven || first != 0 {
            self.material.reset()?;
            self.driven = true;
        }
        Ok(())
    }
    pub fn configure(&mut self, plan: LocalStereoMaterialPlan) -> Result<()> {
        self.material.configure(plan)
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
        self.material.process(pcm, frames, end)
    }
}

#[cfg(all(
    test,
    any(target_os = "linux", target_os = "windows"),
    target_arch = "x86_64"
))]
#[path = "graph_tests.rs"]
mod tests;
