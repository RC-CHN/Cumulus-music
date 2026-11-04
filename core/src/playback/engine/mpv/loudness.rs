use super::{MpvEngine, effect_filter};
use crate::{
    Error, Result,
    effects::loudness::{LoudnessHost, LoudnessSettings, abi::LoudnessStats},
};
use std::path::Path;

impl MpvEngine {
    pub fn attach_loudness(&mut self, library: &Path, settings: LoudnessSettings) -> Result<()> {
        if self.loudness_reserved && self.equalizer_library.as_deref() == Some(library) {
            return self.set_loudness(settings);
        }
        if self.loudness.is_some() {
            return Err(Error::State("loudness adapter is already attached"));
        }
        let host = LoudnessHost::open(library, settings)?;
        let filter = effect_filter("loudness", host.filter_graph());
        // Official processor 1AA50 runs its loudness node before effect nodes.
        self.control(&["af", "pre", &filter])?;
        self.loudness = Some(host);
        self.equalizer_library = Some(library.to_path_buf());
        Ok(())
    }

    /// Attach a verified bypassed node with the first downstream processor,
    /// before any active graph would need to be rewritten. A standalone older
    /// adapter without the optional loudness ABI can still initialize EQ.
    pub(super) fn with_loudness_reservation(
        &self,
        library: &Path,
        filters: String,
    ) -> (String, Option<LoudnessHost>) {
        let Some(host) = self.dry_loudness(library) else {
            return (filters, None);
        };
        (
            format!(
                "{},{}",
                effect_filter("loudness", host.filter_graph()),
                filters
            ),
            Some(host),
        )
    }

    fn dry_loudness(&self, library: &Path) -> Option<LoudnessHost> {
        if self.loudness.is_some() {
            return None;
        }
        LoudnessHost::open(library, LoudnessSettings::default()).ok()
    }

    pub(super) fn adopt_loudness_reservation(&mut self, host: Option<LoudnessHost>) {
        if let Some(host) = host {
            self.loudness = Some(host);
            self.loudness_reserved = true;
        }
    }

    pub(super) fn clear_loudness_reservation(&mut self) {
        self.loudness = None;
        self.loudness_reserved = false;
    }

    pub(super) fn reset_loudness_for_load(&mut self) -> Result<()> {
        if self.equalizer.is_none() && self.parametric.is_none() {
            return self.detach_loudness();
        }
        if self.loudness_reserved
            && self
                .loudness
                .as_ref()
                .is_some_and(|host| host.stats().is_ok_and(|s| s.failed_blocks == 0))
        {
            return Ok(());
        }
        // mpv may still hold the old filter instances after `stop`. A bypass
        // update can ramp from the old track's gain; replace an activated
        // configuration with a fresh dry one before decoding any new source.
        // The controller has already stopped the old source at this point.
        self.detach_loudness()?;
        if let Some(host) = self
            .equalizer_library
            .as_deref()
            .and_then(|path| self.dry_loudness(path))
        {
            let filter = effect_filter("loudness", host.filter_graph());
            self.control(&["af", "pre", &filter])?;
            self.adopt_loudness_reservation(Some(host));
        }
        Ok(())
    }

    pub fn set_loudness(&mut self, settings: LoudnessSettings) -> Result<()> {
        self.loudness
            .as_mut()
            .ok_or(Error::State("loudness adapter is not attached"))?
            .update(settings)?;
        if settings.enabled() {
            self.loudness_reserved = false;
        }
        Ok(())
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
            self.clear_loudness_reservation();
        }
        Ok(())
    }
    pub(super) fn configure_loudness(&mut self, settings: LoudnessSettings) -> Result<bool> {
        let Some(path) = self.equalizer_library.clone() else {
            return Ok(false);
        };
        if self.loudness.as_ref().is_some_and(|h| {
            h.stats().map_or(true, |s| {
                s.failed_blocks != 0
                    || (!self.loudness_reserved && !settings.enabled() && s.active_instances == 0)
            })
        }) {
            self.detach_loudness()?;
        }
        if self.loudness.is_some() {
            if !self.loudness_reserved || settings.enabled() {
                self.set_loudness(settings)?;
            }
        } else if settings.enabled() {
            self.attach_loudness(&path, settings)?;
        }
        Ok(true)
    }
}
