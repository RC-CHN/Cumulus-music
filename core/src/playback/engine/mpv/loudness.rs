use super::MpvEngine;
use crate::{
    Error, Result,
    effects::loudness::{LoudnessHost, LoudnessSettings, abi::LoudnessStats},
};
use std::path::Path;

impl MpvEngine {
    pub fn attach_loudness(&mut self, library: &Path, settings: LoudnessSettings) -> Result<()> {
        if self.loudness.is_some() {
            return Err(Error::State("loudness adapter is already attached"));
        }
        let host = LoudnessHost::open(library, settings)?;
        let graph = host.filter_graph();
        let filter = format!("@cumulus-loudness:lavfi=graph=%{}%{}", graph.len(), graph);
        // Official processor 1AA50 runs its loudness node before effect nodes.
        // Prepending preserves that order even if GEQ was enabled earlier.
        self.control(&["af", "pre", &filter])?;
        self.loudness = Some(host);
        self.equalizer_library = Some(library.to_path_buf());
        Ok(())
    }
    pub fn set_loudness(&mut self, settings: LoudnessSettings) -> Result<()> {
        self.loudness
            .as_mut()
            .ok_or(Error::State("loudness adapter is not attached"))?
            .update(settings)
    }
    pub fn loudness_stats(&self) -> Result<LoudnessStats> {
        self.loudness
            .as_ref()
            .ok_or(Error::State("loudness adapter is not attached"))?
            .stats()
    }
    pub fn detach_loudness(&mut self) -> Result<()> {
        if self.loudness.is_some() {
            self.control(&["af", "remove", "@cumulus-loudness"])?;
            self.loudness = None;
        }
        Ok(())
    }
    pub(super) fn configure_loudness(&mut self, settings: LoudnessSettings) -> Result<bool> {
        let Some(path) = self.equalizer_library.clone() else {
            return Ok(false);
        };
        if self.loudness.as_ref().is_some_and(|h| {
            h.stats().map_or(true, |s| {
                s.failed_blocks != 0 || (!settings.enabled() && s.active_instances == 0)
            })
        }) {
            self.detach_loudness()?;
        }
        if self.loudness.is_some() {
            self.set_loudness(settings)?;
        } else if settings.enabled() {
            self.attach_loudness(&path, settings)?;
        }
        Ok(true)
    }
}
