//! Only the official main bottom bar's explicit forward/back triggers.
use super::MusicClient;
use crate::{
    Result,
    account::SessionToken,
    playback::{
        Step,
        controller::{PlaybackController, PlaybackSnapshot},
        engine::Cancellation,
    },
};

impl MusicClient {
    pub(super) async fn try_minibar_skip(
        &self,
        token: &SessionToken,
        controller: &PlaybackController,
        current: &PlaybackSnapshot,
        direction: Step,
        transport: &Cancellation,
    ) -> Result<Option<PlaybackSnapshot>> {
        transport.check()?;
        if current.fm || current.intelligence.is_some() {
            return Ok(None);
        }
        let Some(verified) = self.player.ordinary_skip.admit(token, transport.clone())? else {
            return Ok(None);
        };
        let outcome = controller
            .verified_minibar_skip(token, current.fence(), direction, verified)
            .await?;
        transport.check()?;
        // An in-flight preflight failure isn't permission to send a second
        // transport intent. Only a decision of Unchanged falls through.
        Ok(outcome.applied)
    }
}
