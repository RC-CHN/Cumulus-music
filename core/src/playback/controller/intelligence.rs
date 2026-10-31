use super::{PlaybackStatus, load::LoadOptions, state::Actor};
use crate::{Error, Result, playback::intelligence::IntelligenceAction};

impl Actor {
    pub(super) async fn intelligence(
        &mut self,
        mut action: IntelligenceAction,
    ) -> Result<Option<LoadOptions>> {
        if let IntelligenceAction::Begin(input) = &mut action {
            self.stamp_quality(std::slice::from_mut(&mut input.seed_resource));
            for entry in &mut input.liked.items {
                self.stamp_quality(std::slice::from_mut(&mut entry.resource));
            }
        }
        if let IntelligenceAction::Complete { resources, .. } = &mut action {
            self.stamp_quality(resources);
        }
        self.save_position()?;
        let before = self.queue.selected();
        let applied = self.queue.intelligence_transition(action)?;
        let after = self.queue.selected();
        if applied.preserve_media {
            let selected = after
                .as_ref()
                .ok_or(Error::State("intelligence lost selected media"))?;
            // Only provenance can change under a retained decoder. Resource,
            // authorization/trial/quality/CUE bounds and entry identity remain
            // byte-for-byte equal. History keeps its original checkpoint.
            if before.as_ref().is_none_or(|old| {
                old.entry_id != selected.entry_id || old.resource != selected.resource
            }) {
                return Err(Error::State("unsafe intelligence media retention"));
            }
            if let Some(loaded) = &mut self.loaded {
                loaded.item = selected.clone();
            }
            if let Some(pending) = &mut self.pending {
                pending.item = selected.clone();
            }
            return Ok(None);
        }
        if after.is_none() {
            self.stop(false).await?;
            return Ok(None);
        }
        if !applied.start_seed
            && matches!(
                self.state,
                PlaybackStatus::Idle | PlaybackStatus::Ended | PlaybackStatus::Error
            )
        {
            // A late HTTP result never restarts explicitly stopped audio.
            // A new Begin remains an explicit play intent.
            self.stop(false).await?;
            return Ok(None);
        }
        Ok(Some(LoadOptions {
            position: Some(0.0),
            recover: true,
            ..Default::default()
        }))
    }
}
