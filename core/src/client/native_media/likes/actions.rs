use super::*;
use crate::client::dto::PodcastCommand;
impl MusicClient {
    /// Explicit read/reconciliation. Never retries a resource write.
    pub async fn native_like_refresh(
        &self,
        selection: &NativeLikeSelection,
    ) -> Result<NativeLikeSnapshot> {
        selection.check(self)?;
        match &selection.resource {
            NativeLikeResource::Song(id) => {
                let service = AccountResources::new(self.session.clone())?;
                if service
                    .membership_for(&selection.token, ResourceKind::Liked, id)?
                    .1
                    .is_some_and(|op| op.phase.pending())
                {
                    service
                        .reconcile_expected(&selection.token, ResourceKind::Liked, id)
                        .await?;
                } else {
                    service
                        .refresh_for(&selection.token, ResourceKind::Liked)
                        .await?;
                }
            }
            NativeLikeResource::Voice(id) => {
                let service = PodcastService::new(self.session.clone())?;
                if service
                    .operation_for(&selection.token, PodcastAction::Like, id)?
                    .is_some_and(|op| op.phase.pending())
                {
                    service
                        .reconcile_for(&selection.token, PodcastAction::Like, id)
                        .await?;
                }
                PodcastLibrary::new(self.session.clone())?
                    .read_for(&selection.token, &PodcastScope::Liked, true)
                    .await?;
            }
        }
        self.native_like_snapshot(selection)
    }
    /// Freeze the displayed resource/desired state before network I/O. Once
    /// accepted, a later track change never retargets or repeats this intent.
    pub async fn native_like_toggle(&self, displayed: &NativeLikeSnapshot) -> Result<()> {
        let current = self.native_like_snapshot(&displayed.selection)?;
        if current.pending || displayed.pending {
            return Err(Error::MutationUncertain);
        }
        let observed = displayed
            .observed
            .ok_or(Error::State("like state is unknown"))?;
        if current.observed != Some(observed) {
            return Err(Error::StaleResponse);
        }
        let epoch = displayed.selection.epoch();
        match &displayed.selection.resource {
            NativeLikeResource::Song(id) => {
                let result = self.set_liked(&epoch, id, !observed).await?;
                if result.value.phase.pending() {
                    return Err(Error::MutationUncertain);
                }
            }
            NativeLikeResource::Voice(id) => {
                let result = self
                    .podcast_command(
                        &epoch,
                        PodcastCommand::Change {
                            action: PodcastAction::Like,
                            id: id.clone(),
                            desired: !observed,
                        },
                    )
                    .await?;
                if result.value.phase.pending() {
                    return Err(Error::MutationUncertain);
                }
            }
        }
        Ok(())
    }
}
