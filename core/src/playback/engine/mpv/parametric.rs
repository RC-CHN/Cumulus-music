use super::MpvEngine;
use crate::{
    Error, Result,
    effects::{
        equalizer::abi::EqualizerStats,
        parametric::{ParametricHost, ParametricPlan},
    },
};
use std::path::Path;

impl MpvEngine {
    /// Explicit native-only PEQ component. No material path or control ID
    /// comes from IPC. The caller owns mono/stereo layout and graph ordering.
    /// This appends one node, not the complete material graph.
    pub fn attach_parametric_equalizer(
        &mut self,
        library: &Path,
        plan: ParametricPlan,
    ) -> Result<()> {
        if self.parametric.is_some() {
            return Err(Error::State("parametric equalizer is already attached"));
        }
        let host = ParametricHost::open(library, plan)?;
        let graph = host.filter_graph();
        let filter = format!("@cumulus-peq:lavfi=graph=%{}%{}", graph.len(), graph);
        self.control(&["af", "add", &filter])?;
        self.parametric = Some(host);
        Ok(())
    }

    pub fn parametric_equalizer_stats(&self) -> Result<EqualizerStats> {
        self.parametric
            .as_ref()
            .ok_or(Error::State("parametric equalizer adapter is not attached"))?
            .stats()
    }

    pub fn detach_parametric_equalizer(&mut self) -> Result<()> {
        if self.parametric.is_some() {
            self.control(&["af", "remove", "@cumulus-peq"])?;
            self.parametric = None;
        }
        Ok(())
    }
}
