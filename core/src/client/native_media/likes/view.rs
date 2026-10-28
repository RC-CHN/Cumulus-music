use super::*;
impl MusicClient {
    pub fn native_like_selection(&self) -> Result<Option<NativeLikeSelection>> {
        let session = self.session.snapshot()?;
        if session.status != SessionStatus::Authenticated
            || session.token.uid.is_none()
            || self.focus.snapshot()?.owner == Some(Owner::Video)
        {
            return Ok(None);
        }
        let Some(controller) = self.player.controller.get() else {
            return Ok(None);
        };
        let value = controller.snapshot();
        if value.account != session.token {
            return Ok(None);
        }
        let Some(selected) = value.selected.as_ref() else {
            return Ok(None);
        };
        let track = super::super::super::mapping::track(&selected.resource)?;
        if track.local || track.missing {
            return Ok(None);
        }
        let resource = match track.voice_id {
            Some(id) => NativeLikeResource::Voice(crate::protocol::api::identifier(&json!(id))?),
            None => NativeLikeResource::Song(crate::protocol::api::identifier(&json!(track.id))?),
        };
        self.session.check_generation(&session.token)?;
        Ok(Some(NativeLikeSelection {
            token: session.token,
            entry_id: selected.entry_id.clone(),
            resource,
        }))
    }
    /// Read persistence on a worker thread; no remote request or decoder startup.
    pub fn native_like_snapshot(
        &self,
        selection: &NativeLikeSelection,
    ) -> Result<NativeLikeSnapshot> {
        selection.check(self)?;
        let (observed, pending) = match &selection.resource {
            NativeLikeResource::Song(id) => {
                let (observed, operation) = AccountResources::new(self.session.clone())?
                    .membership_for(&selection.token, ResourceKind::Liked, id)?;
                (
                    observed,
                    operation.is_some_and(|value| value.phase.pending()),
                )
            }
            NativeLikeResource::Voice(id) => {
                let library = PodcastLibrary::new(self.session.clone())?;
                let cached = library.cached_for(&selection.token, &PodcastScope::Liked)?;
                let op = PodcastService::new(self.session.clone())?.operation_for(
                    &selection.token,
                    PodcastAction::Like,
                    id,
                )?;
                let observed = cached
                    .map(|s| s.voices.iter().any(|voice| voice.id == *id))
                    .or_else(|| {
                        op.as_ref()
                            .filter(|v| !v.phase.pending())
                            .and_then(|v| v.observed)
                    });
                (observed, op.is_some_and(|value| value.phase.pending()))
            }
        };
        selection.check(self)?;
        Ok(NativeLikeSnapshot {
            selection: selection.clone(),
            observed,
            pending,
        })
    }
}
