mod trial;
mod view;
use super::{MusicClient, PlaybackCommand, PlaybackSnapshot};
use crate::{
    Error, Result,
    account::SessionToken,
    playback::quality::{QualityFormat, QualityReader},
};
pub use trial::{QualityTrialCommand, QualityTrialOutcome, QualityTrialView};
pub use view::{QualityChoiceView, QualityDetailsView};

impl MusicClient {
    pub(super) async fn quality_context(
        &self,
        epoch: &str,
        play_id: &str,
    ) -> Result<(SessionToken, PlaybackSnapshot, String, Option<i32>)> {
        let token = self.player_token(epoch)?;
        let current = self
            .player
            .controller()
            .await?
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        if current.play_id.as_deref() != Some(play_id) || !current.quality_preferences_apply {
            return Err(Error::StaleResponse);
        }
        if !matches!(
            current.state,
            crate::playback::controller::PlaybackStatus::Playing
                | crate::playback::controller::PlaybackStatus::Paused
                | crate::playback::controller::PlaybackStatus::Buffering
        ) {
            return Err(Error::State("quality requires loaded online track"));
        }
        let resource = &current
            .selected
            .as_ref()
            .ok_or(Error::State("no selected song"))?
            .resource;
        let song = crate::protocol::api::identifier(&resource["id"])?;
        let trial_mode = resource
            .get("trial_mode")
            .filter(|v| !v.is_null())
            .map(|v| {
                v.as_i64()
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or(Error::InvalidInput("invalid quality trial scene"))
            })
            .transpose()?;
        Ok((token, current, song, trial_mode))
    }
    pub async fn player_quality_details(
        &self,
        epoch: &str,
        play_id: &str,
    ) -> Result<QualityDetailsView> {
        let (token, current, song, trial_mode) = self.quality_context(epoch, play_id).await?;
        let information = QualityReader::new(self.session.clone())
            .read_for(&token, &song, trial_mode, QualityFormat::Surround51)
            .await?;
        self.session.check_generation(&token)?;
        let latest = self
            .player
            .controller()
            .await?
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        if latest.play_id != current.play_id
            || latest.quality_preferences != current.quality_preferences
        {
            return Err(Error::StaleResponse);
        }
        let records = crate::playback::quality::trials::QualityTrials::new(self.session.clone())?
            .records_for(&token);
        self.session.check_generation(&token)?;
        let (records, damaged) = match records {
            Ok(records) => (records, false),
            Err(_) => (vec![], true),
        };
        view::project(
            epoch,
            play_id,
            &song,
            &latest,
            &information,
            records,
            damaged,
        )
    }
}
