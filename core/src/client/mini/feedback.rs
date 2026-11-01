//! FM menus keep the exact second-window target and reuse the main FM journal.
use super::*;
use crate::{
    client::fm::FmCommand,
    fm::{FmReasons, FmSource},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MiniFmFeedbackView {
    pub epoch: String,
    pub entry_id: String,
    pub title: String,
    pub pending: bool,
}

impl MusicClient {
    async fn mini_feedback_target(
        &self,
        displayed: &MiniSnapshot,
        refresh: bool,
    ) -> Result<(FmSource, MiniFmFeedbackView)> {
        displayed.audio(self)?;
        if displayed.view.mode_family != Some(ModeFamily::Fm) {
            return Err(Error::InvalidInput("mini feedback requires FM ownership"));
        }
        let expected = &displayed
            .mode_context
            .as_ref()
            .ok_or(Error::StaleResponse)?
            .expected;
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let (state, queue) = controller.queue_with_state(&displayed.token).await?;
        if &state.fence() != expected {
            return Err(Error::StaleResponse);
        }
        let fm = self.player.fm().await?;
        let source = fm
            .active_source_for(&displayed.token, &queue)?
            .ok_or(Error::QueueChanged)?;
        let entry_id = queue.current.ok_or(Error::QueueChanged)?;
        if refresh {
            // A retry reads remote membership and repairs only local caches;
            // it never starts/refills FM or applies an uncertain operation.
            fm.service().refresh_blacklist_for(&displayed.token).await?;
            displayed.check(self)?;
            fm.service().refresh_trash_for(&displayed.token).await?;
        }
        let state = self
            .fm_view(&displayed.view.epoch, source.clone(), false)
            .await?
            .value;
        displayed.check(self)?;
        Ok((
            source,
            MiniFmFeedbackView {
                epoch: displayed.view.epoch.clone(),
                entry_id,
                title: displayed
                    .view
                    .item
                    .as_ref()
                    .ok_or(Error::StaleResponse)?
                    .title
                    .clone(),
                pending: state.operations.iter().any(|op| op.kind != "skip"),
            },
        ))
    }

    pub async fn mini_fm_feedback_view(
        &self,
        displayed: &MiniSnapshot,
        refresh: bool,
    ) -> Result<MiniFmFeedbackView> {
        self.mini_feedback_target(displayed, refresh)
            .await
            .map(|(_, view)| view)
    }

    pub async fn mini_fm_feedback_reasons(&self, displayed: &MiniSnapshot) -> Result<FmReasons> {
        let (source, view) = self.mini_feedback_target(displayed, false).await?;
        if view.pending {
            return Err(Error::MutationUncertain);
        }
        let result = self
            .fm_reasons(&view.epoch, source, &view.entry_id)
            .await?
            .value;
        displayed.check(self)?;
        Ok(result)
    }

    pub async fn mini_fm_feedback(
        &self,
        displayed: &MiniSnapshot,
        reason_key: Option<String>,
    ) -> Result<()> {
        let (source, view) = self.mini_feedback_target(displayed, false).await?;
        if view.pending {
            return Err(Error::MutationUncertain);
        }
        let command = match reason_key {
            Some(reason_key) => FmCommand::Feedback {
                entry_id: view.entry_id,
                reason_key,
            },
            None => FmCommand::Trash {
                entry_id: view.entry_id,
            },
        };
        // FmPlayer checks the current entry under its gate again, journals the
        // write and advances only its own queue. No generic artist ID from UI.
        self.fm_command(&view.epoch, source, command).await?;
        Ok(())
    }
}
