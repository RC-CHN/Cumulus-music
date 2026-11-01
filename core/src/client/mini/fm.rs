//! Read-only projection of a durable FM repeat which the actor did not apply.
use super::*;
use crate::playback::{PlaybackQueue, controller::PlaybackSnapshot};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MiniFmModePending {
    pub mode: PlaybackMode,
    pub revision: String,
}
impl MusicClient {
    pub(super) fn mini_fm_mode_pending(
        &self,
        token: &SessionToken,
        state: Option<&PlaybackSnapshot>,
    ) -> Result<Option<MiniFmModePending>> {
        let Some(state) = state.filter(|state| state.fm) else {
            return Ok(None);
        };
        let owner = self
            .session
            .with_token(token, |owner| Ok(owner.to_owned()))?;
        let actual = PlaybackQueue::new(Some(self.registry.store()), &owner)?;
        // A newly opened reader's in-memory revision starts at zero. The
        // durable queue is only a recovery hint, not a command authority;
        // reject a mismatched projection and retain the actor fence for
        // every eventual write.
        if actual.mode() != state.mode
            || actual.selected() != state.selected
            || actual.len() != state.queue_len
            || actual.visible_len() != state.visible_queue_len
            || actual.is_fm() != state.fm
        {
            return Err(Error::StaleResponse);
        }
        let queue = actual.snapshot();
        let pending = if let Some(source) = crate::fm::player::source(&queue)? {
            let service = crate::fm::FmService::new(self.session.clone())?;
            service.queue_for(token, &source)?.and_then(|desired| {
                crate::fm::player::unapplied_mode(&desired, &queue).map(|mode| MiniFmModePending {
                    mode,
                    revision: desired.revision.to_string(),
                })
            })
        } else {
            None
        };
        self.session.with_token(token, |_| Ok(pending))
    }
}
