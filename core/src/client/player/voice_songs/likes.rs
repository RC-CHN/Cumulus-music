//! M/3160 like intent from one ordinary row of the visible episode card.
use super::{MusicClient, seed, stale_credentials};
use crate::{
    Error, Result,
    account::resources::AccountResources,
    client::dto::{AccountData, LikeResult},
};
use serde_json::json;

impl MusicClient {
    pub async fn player_voice_song_like(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
        index: usize,
        liked: bool,
    ) -> Result<AccountData<LikeResult>> {
        let token = self.player_token(epoch)?;
        let host = &self.player.voice_songs;
        let page = host.page(&token, play_id, receipt)?;
        let api = page.api.upgrade().ok_or(Error::StaleResponse)?;
        let room = self.together_detached_guard(&token);
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let check = || {
            host.check_action_scope(self, &page, &api, &room)?;
            if api
                .song_like_scope_revision(true)
                .map_err(stale_credentials)?
                .as_deref()
                != Some(page.revision.as_str())
                || seed(&controller.snapshot(), &token, play_id).ok().as_ref() != Some(&page.seed)
            {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        check()?;
        let row = page
            .rows
            .get(index)
            .ok_or(Error::InvalidInput("invalid vinyl episode song row"))?;
        // Only the card’s ordinary song ID is admitted. The current episode
        // and its backing media do not select a membership target. Audio
        // playback eligibility is independent of song collection.
        let record = AccountResources::new(self.session.clone())?
            .set_song_like_guarded_for(&token, &json!(row.track.id), liked, &check)
            .await
            .map_err(stale_credentials)?;
        // An already dispatched mutation remains in the account journal even
        // if the page or credential changed while awaiting its read-back.
        check()?;
        Ok(AccountData {
            epoch: token.generation.to_string(),
            value: LikeResult::from_membership(record),
        })
    }
}
