//! Persist the coordinator alongside an empty FM queue. The old source marker
//! is read only to migrate a pre-existing empty queue, never to claim a new one.
use super::*;
use crate::{fm::FmSource, playback::mode::ModeFamily};

impl QueueSnapshot {
    pub(super) fn validate_fm_identity(&self) -> Result<()> {
        if let Some(source) = &self.fm_source {
            source.validate()?;
            if !ModeFamily::Fm.supports(self.mode) {
                return Err(Error::State("invalid FM queue mode"));
            }
            for entry in &self.items {
                if entry.source["kind"] != "fm"
                    || serde_json::from_value::<FmSource>(entry.source["source"].clone())?
                        != *source
                {
                    return Err(Error::State("mixed FM queue sources"));
                }
            }
        }
        Ok(())
    }
}

impl PlaybackQueue {
    pub(super) fn load_current(
        store: Option<&StateStore>,
        owner: &str,
        raw: Option<&Value>,
    ) -> Result<QueueSnapshot> {
        let mut state = QueueSnapshot::load(raw)?;
        if state.items.is_empty()
            && raw.is_some_and(|v| v.get("fm_source").is_none())
            && ModeFamily::Fm.supports(state.mode)
            && let Some(store) = store
            && store.get(owner, "playback", FM_BACKUP_QUEUE)?.is_some()
            && let Some(source) = store.get(owner, "fm-player-v1", "source")?
        {
            state.fm_source = Some(serde_json::from_value(source)?);
            state.validate()?;
        }
        Ok(state)
    }
}
