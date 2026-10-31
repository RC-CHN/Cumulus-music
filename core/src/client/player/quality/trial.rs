use super::super::{MusicClient, PlaybackCommand, PlayerAction, PlayerView, project};
use crate::{
    Error, Result,
    client::ClientError,
    membership::Quality,
    playback::{
        engine::Cancellation,
        quality::{
            QualityAccess, QualityRight,
            trials::{QualityTrial, QualityTrialPhase, QualityTrials, TrialConsent},
        },
    },
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualityTrialCommand {
    Start {
        play_id: String,
        details_revision: String,
        level: Quality,
    },
    UseActive {
        play_id: String,
        details_revision: String,
        level: Quality,
    },
    Reconcile {
        right: QualityRight,
        operation: String,
    },
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityTrialView {
    pub right: QualityRight,
    pub operation: String,
    pub revision: String,
    pub phase: QualityTrialPhase,
    pub acknowledged: bool,
    pub can_use: bool,
    pub read_failed: bool,
    pub remaining_ms: Option<u64>,
}
impl From<QualityTrial> for QualityTrialView {
    fn from(record: QualityTrial) -> Self {
        Self {
            right: record.right,
            operation: record.operation.clone(),
            revision: record.revision.to_string(),
            phase: record.phase,
            acknowledged: record.acknowledged,
            can_use: record.can_use(),
            read_failed: record.read_failed,
            remaining_ms: record
                .observed
                .filter(|_| !record.read_failed)
                .and_then(|r| r.remaining_ms),
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityTrialOutcome {
    pub trial: QualityTrialView,
    pub playback: Option<PlayerView>,
    /// Quota may have been consumed even if a later playback/target check failed.
    /// Hosts must retry UseActive, never repeat Start as an audio-error retry.
    pub playback_error: Option<ClientError>,
}
impl MusicClient {
    pub async fn player_quality_trial(
        &self,
        epoch: &str,
        command: QualityTrialCommand,
    ) -> Result<QualityTrialOutcome> {
        let token = self.player_token(epoch)?;
        let trials = QualityTrials::new(self.session.clone())?;
        let (play_id, details_revision, level, start) = match command {
            QualityTrialCommand::Reconcile { right, operation } => {
                if trials
                    .record_for(&token, right)?
                    .is_none_or(|r| r.operation != operation)
                {
                    return Err(Error::StaleResponse);
                }
                let record = trials.observe_for(&token, right).await?;
                return Ok(QualityTrialOutcome {
                    trial: record.into(),
                    playback: None,
                    playback_error: None,
                });
            }
            QualityTrialCommand::Start {
                play_id,
                details_revision,
                level,
            } => (play_id, details_revision, level, true),
            QualityTrialCommand::UseActive {
                play_id,
                details_revision,
                level,
            } => (play_id, details_revision, level, false),
        };
        let right = QualityRight::for_quality(level)
            .ok_or(Error::InvalidInput("quality has no usage trial"))?;
        let intent = self.player.selection.load(Ordering::Acquire);
        let (_, before, _, _) = self.quality_context(epoch, &play_id).await?;
        if before.quality_preferences.damaged {
            return Err(Error::State("listening quality preference damaged"));
        }
        let target = PlayerAction::QualityFor {
            play_id: play_id.clone(),
            expected_revision: before.quality_preferences.preferences.revision.clone(),
            level,
        };
        self.together_validate_action(&token, &target)?;
        let details = self.player_quality_details(epoch, &play_id).await?;
        let choice = details
            .choices
            .iter()
            .find(|c| c.quality == level)
            .ok_or(Error::State("quality trial offer changed"))?;
        if details.revision != details_revision
            || choice.access
                != if start {
                    QualityAccess::TrialEligible
                } else {
                    QualityAccess::TrialActive
                }
        {
            return Err(Error::State("quality trial offer changed"));
        }
        let (_, latest, _, _) = self.quality_context(epoch, &play_id).await?;
        if latest.fence() != before.fence()
            || latest.quality_preferences != before.quality_preferences
            || self.player.selection.load(Ordering::Acquire) != intent
        {
            return Err(Error::StaleResponse);
        }
        let record = if start {
            trials
                .start_for(
                    &token,
                    right,
                    choice.remaining_ms,
                    TrialConsent::ExplicitStart,
                )
                .await?
        } else {
            trials.adopt_for(&token, right).await?
        };
        if !record.can_use() {
            return Ok(QualityTrialOutcome {
                trial: record.into(),
                playback: None,
                playback_error: None,
            });
        }
        let result = async {
            let (_, latest, _, _) = self.quality_context(epoch, &play_id).await?;
            if latest.quality_preferences != before.quality_preferences
                || self.player.selection.load(Ordering::Acquire) != intent
            {
                return Err(Error::StaleResponse);
            }
            let selection = self.player.selection.clone();
            let state = self
                .player
                .controller()
                .await?
                .command_guarded(
                    &token,
                    PlaybackCommand::TrialQuality {
                        right,
                        operation: record.operation.clone(),
                    },
                    before.fence(),
                    Cancellation::new(move || selection.load(Ordering::Acquire) == intent),
                )
                .await?;
            self.session.check_generation(&token)?;
            self.together_signal(&token, crate::client::together::signal(&target));
            project(&state, &token)
        }
        .await;
        self.session.check_generation(&token)?;
        let (playback, playback_error) = match result {
            Ok(p) => (Some(p), None),
            Err(e) => (None, Some(e.into())),
        };
        Ok(QualityTrialOutcome {
            trial: record.into(),
            playback,
            playback_error,
        })
    }
}
