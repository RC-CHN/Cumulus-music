//! Search AudioCard overlays: M3095 -> M537 -> M3085/M404.
use super::{
    MusicClient, PlaybackCommand,
    podcast_recommended::{RadioPlaybackPlan, same_radio_transport},
};
use crate::{Error, Result, account::SessionToken, playback::engine::Cancellation};
use serde_json::json;

impl MusicClient {
    pub(super) async fn search_podcast_selection(
        &self,
        token: &SessionToken,
        receipt: &str,
        id: &str,
    ) -> Result<(PlaybackCommand, Cancellation)> {
        if self.login_pending()? {
            return Err(Error::StaleResponse);
        }
        let api = self.session.native_api(token)?;
        let (radio, observed) = self.search_podcast_playback_source(token, &api, receipt, id)?;
        let room = self.together_detached_guard(token);
        let pending = self.auth.pending_guard();
        let guard = Cancellation::new(move || {
            !observed.is_cancelled() && !room.is_cancelled() && pending().is_ok()
        });
        guard.check()?;
        let controller = self.player.controller().await?;
        if let Some(command) = same_radio_transport(&controller.snapshot(), token, id) {
            return Ok((command, guard));
        }
        let RadioPlaybackPlan {
            resources,
            index,
            position,
        } = self
            .prepare_radio_playback(token, &api, &radio, &guard, true)
            .await?;
        guard.check()?;
        // Authorize the chosen episode before replacing the old queue. Keep
        // the fresh source native and pass it only to the exact fenced load.
        let first_source = controller
            .source_resolver()
            .prepare_scoped_voice(&resources[index], token, &api, &guard)
            .await?;
        guard.check()?;
        let sequential = !matches!(radio.raw["categoryId"].as_i64(), Some(3001 | 10002 | 2001));
        Ok((
            PlaybackCommand::ReplacePodcastCollection {
                resources,
                index,
                position,
                sequential,
                first_source: Some(first_source),
                source: json!({"kind":"search_podcast_card","id":radio.id,"scene":"unknown"}),
            },
            guard,
        ))
    }
}
