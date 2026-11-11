use super::*;
use crate::client::dto::{PlayerAction, PlayerView};
use crate::client::focus::FocusSnapshot;
use crate::playback::controller::PlaybackFence;
use std::time::Duration;

// A background room tick can briefly own the same gate. Keep the UI
// responsive if a real room request holds it longer, without sending a write.
const ROOM_MODE_GATE_WAIT: Duration = Duration::from_millis(250);

/// Includes an empty FM queue, which has no system media/transport target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MiniModeContext {
    pub focus: FocusSnapshot,
    pub expected: PlaybackFence,
}
impl MiniModeContext {
    pub fn check(&self, client: &MusicClient, token: &SessionToken) -> Result<()> {
        let state = client
            .player
            .controller
            .get()
            .ok_or(Error::StaleResponse)?
            .snapshot();
        client.session.check_generation(token)?;
        client.focus.check_snapshot(self.focus)?;
        if state.account != *token || state.fence() != self.expected {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

/// Exact actor acknowledgement, rather than a later read of possibly new media.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MiniPlaybackReceipt {
    pub epoch: String,
    pub queue_revision: String,
    pub play_id: Option<String>,
    pub mode: PlaybackMode,
    pub fm: bool,
    pub intelligence: Option<crate::playback::intelligence::IntelligencePhase>,
    pub intelligence_operation: Option<String>,
}
impl From<PlayerView> for MiniPlaybackReceipt {
    fn from(v: PlayerView) -> Self {
        Self {
            epoch: v.epoch,
            queue_revision: v.queue_revision,
            play_id: v.play_id,
            mode: v.mode,
            fm: v.fm,
            intelligence: v.intelligence,
            intelligence_operation: v.intelligence_operation,
        }
    }
}
impl MusicClient {
    pub async fn mini_mode(
        &self,
        displayed: &MiniSnapshot,
        mode: PlaybackMode,
    ) -> Result<MiniPlaybackReceipt> {
        let _room = tokio::time::timeout(ROOM_MODE_GATE_WAIT, self.together.gate.lock())
            .await
            .map_err(|_| Error::MutationBusy)?;
        // Keep the originally displayed controls; any intervening room,
        // account or queue change invalidates them instead of retargeting.
        displayed.check(self)?;
        let family = displayed
            .view
            .mode_family
            .ok_or(Error::InvalidInput("mini mode unavailable"))?;
        if !family.supports(mode) {
            return Err(Error::InvalidInput(
                "mode unavailable in mini playback context",
            ));
        }
        if family == ModeFamily::Fm {
            let context = displayed
                .mode_context
                .as_ref()
                .ok_or(Error::StaleResponse)?;
            let coordinator = self.player.fm().await?;
            context.check(self, &displayed.token)?;
            let state = coordinator
                .repeat_checked_for(
                    &displayed.token,
                    if mode == PlaybackMode::SingleLoop {
                        crate::fm::FmRepeat::One
                    } else {
                        crate::fm::FmRepeat::Infinite
                    },
                    self.player
                        .selection
                        .load(std::sync::atomic::Ordering::Acquire),
                    context.expected.clone(),
                )
                .await?;
            return super::super::player::project(&state, &displayed.token).map(Into::into);
        }
        let target = displayed.target.as_ref().ok_or(Error::StaleResponse)?;
        self.player_action_checked(
            &displayed.view.epoch,
            PlayerAction::Mode { mode },
            Some(target),
        )
        .await
        .map(Into::into)
    }
}
