//! Single held-source graph lifetime. Preparing a decoder is not PCM use.
#[cfg(test)]
use crate::effects::{
    dsp::LimiterStage,
    material::{ModernLimiterPlan, ModernLimiterStages},
};
#[cfg(test)]
use crate::playback::engine::{Cancellation, stereo_pcm::TestModernCommand};
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
    #[cfg(test)]
    actor_modern: Option<TestModernLease>,
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
            #[cfg(test)]
            actor_modern: None,
        })
    }
    pub fn new_local(format: Format, plan: LocalEqualizerMaterialPlan) -> Result<Self> {
        Ok(Self {
            material: Material::Local(LocalEqualizerStereoMaterial::new(format, plan)?),
            driven: false,
            #[cfg(test)]
            actor_modern: None,
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
                plan: plan.clone(),
                active: true,
                failed: false,
            })),
            driven: false,
            actor_modern: None,
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
            #[cfg(test)]
            (Material::Modern(graph), StereoGraphControl::SetModernMaterialActive(active)) => {
                graph.apply(active)
            }
            #[cfg(test)]
            (Material::Local(graph), StereoGraphControl::SetModernMaterialActive(active)) => {
                graph.set_modern_material_active(active)
            }
            #[cfg(test)]
            (material, StereoGraphControl::InitialModernFromLocal { plan, outcome }) => {
                let result = match material {
                    Material::Local(graph) => graph.apply_initial_modern(&plan),
                    _ => Err(Error::State("initial modern effect needs a local graph")),
                };
                // A rejected transition leaves the old graph running. The
                // fixture needs a separate success/failure control receipt.
                let _ = outcome.try_send(result);
                Ok(())
            }
            #[cfg(test)]
            (Material::Local(graph), StereoGraphControl::ActorModern { command, outcome }) => {
                let result = TestModernLease::apply(&mut self.actor_modern, graph, command);
                let _ = outcome.try_send(result);
                Ok(())
            }
            _ => Err(Error::InvalidInput(
                "control belongs to another stereo graph",
            )),
        }
    }
    pub fn initial_modern_ready(&self) -> bool {
        match &self.material {
            Material::Local(graph) => graph.initial_modern_ready(),
            _ => false,
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
        #[cfg(test)]
        if let (Material::Local(graph), Some(lease)) = (&mut self.material, &mut self.actor_modern)
            && lease.authority.is_cancelled()
        {
            // An observed revocation cannot authorize another active quantum.
            // Existing PCM/native buffering retains its original timeline.
            lease.close(graph)?;
        }
        match &mut self.material {
            Material::Cold(graph) => graph.process(pcm, frames, end),
            Material::Local(graph) => graph.process(pcm, frames, end),
            #[cfg(test)]
            Material::Modern(graph) => graph.process(pcm, frames, end),
        }
    }
}

#[cfg(test)]
struct TestModernLease {
    owner: uuid::Uuid,
    authority: Cancellation,
    active: bool,
}

#[cfg(test)]
impl TestModernLease {
    fn apply(
        current: &mut Option<Self>,
        graph: &mut LocalEqualizerStereoMaterial,
        command: TestModernCommand,
    ) -> Result<()> {
        match command {
            TestModernCommand::Attach {
                owner,
                plan,
                authority,
            } => {
                if current.is_some() {
                    return Err(Error::State("synthetic modern graph already has an owner"));
                }
                authority.check()?;
                graph.apply_initial_modern(&plan)?;
                *current = Some(Self {
                    owner,
                    authority,
                    active: true,
                });
            }
            TestModernCommand::Close(owner) => {
                current
                    .as_mut()
                    .filter(|lease| lease.owner == owner)
                    .ok_or(Error::StaleResponse)?
                    .close(graph)?;
            }
            TestModernCommand::ReturnToGeq(owner, plan) => {
                if current.as_ref().is_none_or(|lease| lease.owner != owner) {
                    return Err(Error::StaleResponse);
                }
                graph.return_to_legacy_geq(plan)?;
                *current = None;
            }
        }
        Ok(())
    }

    fn close(&mut self, graph: &mut LocalEqualizerStereoMaterial) -> Result<()> {
        if self.active {
            graph.set_modern_material_active(false)?;
            self.active = false;
        }
        Ok(())
    }
}

#[cfg(test)]
struct ModernBlocks {
    format: Format,
    stages: ModernLimiterStages,
    limiter: LimiterStage,
    plan: ModernLimiterPlan,
    active: bool,
    failed: bool,
}

#[cfg(test)]
impl ModernBlocks {
    fn apply(&mut self, active: bool) -> Result<()> {
        if self.failed {
            return Err(Error::State("held modern limiter graph is quarantined"));
        }
        if self.active == active {
            return Ok(());
        }
        let replacement = self.plan.prepare_control(self.format, active)?;
        if let Err(error) = self.stages.apply_control(&self.plan, active) {
            self.failed = true;
            return Err(error);
        }
        if let Some(stage) = replacement {
            self.limiter = stage;
        }
        self.active = active;
        Ok(())
    }

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
