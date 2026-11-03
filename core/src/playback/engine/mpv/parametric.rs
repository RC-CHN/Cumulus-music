use super::{MpvEngine, effect_filter};
use crate::{
    Error, Result,
    effects::{
        equalizer::{EqualizerHost, EqualizerSettings, abi::EqualizerStats},
        parametric::{ParametricHost, ParametricPlan},
    },
};
use std::path::Path;

impl MpvEngine {
    /// Explicit native-only PEQ component. No material path or control ID
    /// comes from IPC. The caller owns mono/stereo layout. This is not a
    /// complete material graph.
    pub fn attach_parametric_equalizer(
        &mut self,
        library: &Path,
        plan: ParametricPlan,
    ) -> Result<()> {
        if self.parametric.is_some() {
            return Err(Error::State("parametric equalizer is already attached"));
        }
        let host = ParametricHost::open(library, plan)?;
        let peq = effect_filter("peq", host.filter_graph());
        if self.equalizer.is_some() {
            self.control(&["af", "add", &peq])?;
        } else {
            // Reserve a dry GEQ before the first PEQ. Later GEQ adjustment
            // updates that node in place rather than rewriting a running graph.
            let path = self
                .equalizer_library
                .clone()
                .unwrap_or_else(|| library.into());
            let geq = EqualizerHost::open(&path, EqualizerSettings::default())?;
            let filters = format!("{},{}", effect_filter("geq", geq.filter_graph()), peq);
            self.control(&["af", "add", &filters])?;
            self.equalizer = Some(geq);
            self.equalizer_library = Some(path);
            self.equalizer_reserved = true;
        }
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
            let labels = if self.equalizer_reserved {
                "@cumulus-peq,@cumulus-geq"
            } else {
                "@cumulus-peq"
            };
            self.control(&["af", "remove", labels])?;
            self.parametric = None;
            if self.equalizer_reserved {
                self.equalizer = None;
                self.equalizer_reserved = false;
            }
        }
        Ok(())
    }
}
