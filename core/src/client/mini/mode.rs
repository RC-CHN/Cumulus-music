use super::*;
use crate::client::dto::{PlayerAction, PlayerView};

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
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
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
