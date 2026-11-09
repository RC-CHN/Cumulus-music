use super::{MusicClient, selected_song};
use crate::{
    Error, Result,
    playback::{
        QueueSnapshot,
        controller::{PlaybackController, PlaybackSnapshot},
    },
    protocol::api::{Api, identifier},
};
use serde_json::Value;
use std::sync::Arc;

impl MusicClient {
    pub(super) async fn fm_chorus(
        &self,
        controller: &PlaybackController,
        current: &PlaybackSnapshot,
        queue: &QueueSnapshot,
        play_id: &str,
        song: &Value,
        api: &Arc<dyn Api>,
    ) -> Result<Option<f64>> {
        let token = &current.account;
        let room = self.together_detached_guard(token);
        let Some(source) = crate::fm::player::source(queue)? else {
            return Ok(None);
        };
        if room.is_cancelled() {
            return Ok(None);
        }
        let service = self.player.fm_service().await?;
        let Some(receipt) = service.chorus_receipt(token, api, &source)? else {
            return Ok(None);
        };
        receipt.wait().await?;
        let (latest, actual) = controller.queue_with_state(token).await?;
        room.check()?;
        if !latest.fm
            || latest.queue_revision != current.queue_revision
            || latest.selected != current.selected
            || selected_song(&latest, play_id)? != Some(song)
            || crate::fm::player::source(&actual)?.as_ref() != Some(&source)
        {
            return Err(Error::StaleResponse);
        }
        service.chorus_point(&receipt, &identifier(song)?)
    }
}
