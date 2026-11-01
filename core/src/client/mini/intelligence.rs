use super::*;
use crate::client::dto::{IntelligenceCommand, IntelligenceOrigin};
use serde::Deserialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MiniIntelligenceRead {
    pub operation_id: String,
    pub preparing: bool,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MiniIntelligenceCommand {
    Start { operation_id: String },
    Continue { operation_id: String },
}
impl MiniIntelligenceCommand {
    pub fn operation_id(&self) -> &str {
        match self {
            Self::Start { operation_id } | Self::Continue { operation_id } => operation_id,
        }
    }
}
impl MusicClient {
    /// A local display hint only. Current Start independently reads the actual
    /// owned favourite from the service; this cache never grants entitlement.
    pub(super) fn mini_heart_hint(
        &self,
        token: &SessionToken,
        mode: Option<PlaybackMode>,
    ) -> Result<bool> {
        if mode.is_none() {
            return Ok(false);
        }
        let liked = self.intelligence_liked_hint(token)?;
        self.session.with_token(token, |_| {
            let Some(state) = self
                .player
                .controller
                .get()
                .map(|v| v.snapshot())
                .filter(|v| v.account == *token && mode.is_some() && !v.fm)
            else {
                return Ok(false);
            };
            let Some(item) = state
                .selected
                .as_ref()
                .filter(|v| v.source["kind"] == "playlist")
            else {
                return Ok(false);
            };
            Ok(liked
                .as_deref()
                .is_some_and(|id| item.source["id"].as_str() == Some(id)))
        })
    }
    pub async fn mini_intelligence(
        &self,
        displayed: &MiniSnapshot,
        command: MiniIntelligenceCommand,
    ) -> Result<MiniPlaybackReceipt> {
        let target = displayed.audio(self)?;
        if matches!(
            displayed.view.mode_family,
            None | Some(ModeFamily::Together | ModeFamily::Fm)
        ) {
            return Err(Error::InvalidInput("mini intelligence unavailable"));
        }
        let revision = target
            .audio(
                &self
                    .player
                    .controller
                    .get()
                    .ok_or(Error::StaleResponse)?
                    .snapshot(),
            )?
            .queue_revision
            .to_string();
        let command = match command {
            MiniIntelligenceCommand::Start { operation_id } => {
                if !displayed.view.can_intelligence
                    || displayed.view.mode_family != Some(ModeFamily::Ordinary)
                {
                    return Err(Error::State("intelligence requires the liked playlist"));
                }
                IntelligenceCommand::Start {
                    operation_id,
                    expected_queue_revision: revision,
                    origin: IntelligenceOrigin::Current,
                }
            }
            MiniIntelligenceCommand::Continue { operation_id } => {
                if displayed.view.intelligence
                    != Some(crate::playback::intelligence::IntelligencePhase::Seeded)
                {
                    return Err(Error::StaleResponse);
                }
                IntelligenceCommand::Continue {
                    operation_id,
                    expected_queue_revision: revision,
                }
            }
        };
        self.player_intelligence_checked(&displayed.view.epoch, command, Some(target))
            .await
            .map(Into::into)
    }
}
