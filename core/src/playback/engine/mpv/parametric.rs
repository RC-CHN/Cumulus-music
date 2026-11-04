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
            let (filters, loudness) = self.with_loudness_reservation(&path, filters);
            self.control(&["af", "add", &filters])?;
            self.adopt_loudness_reservation(loudness);
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
            let dry_loudness =
                self.loudness_reserved && (self.equalizer_reserved || self.equalizer.is_none());
            let labels = match (self.equalizer_reserved, dry_loudness) {
                (true, true) => "@cumulus-peq,@cumulus-geq,@cumulus-loudness",
                (true, false) => "@cumulus-peq,@cumulus-geq",
                (false, true) => "@cumulus-peq,@cumulus-loudness",
                (false, false) => "@cumulus-peq",
            };
            self.control(&["af", "remove", labels])?;
            if dry_loudness {
                self.clear_loudness_reservation();
            }
            self.parametric = None;
            if self.equalizer_reserved {
                self.equalizer = None;
                self.equalizer_reserved = false;
            }
        }
        Ok(())
    }
}
