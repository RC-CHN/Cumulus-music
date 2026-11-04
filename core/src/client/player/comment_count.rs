//! Optional comment badge for the current online media resource.
use super::{MusicClient, PlaybackCommand, PlaybackSnapshot, PlayerResourceKind, identity};
use crate::{
    Error, Result,
    comments::{CommentApi, ResourceKind},
    protocol::api::identifier,
};
use std::time::Duration;

fn selected_resource(
    snapshot: &PlaybackSnapshot,
    play_id: &str,
) -> Result<Option<(ResourceKind, String)>> {
    if snapshot.play_id.as_deref() != Some(play_id) {
        return Err(Error::StaleResponse);
    }
    Ok(snapshot.selected.as_ref().and_then(|entry| {
        let (kind, field) = match identity::kind(&entry.resource) {
            PlayerResourceKind::Catalog => (ResourceKind::Track, "id"),
            PlayerResourceKind::Voice => (ResourceKind::Voice, "voice_id"),
            _ => return None,
        };
        identifier(&entry.resource[field]).ok().map(|id| (kind, id))
    }))
}

impl MusicClient {
    pub async fn player_comment_count(&self, epoch: &str, play_id: &str) -> Result<Option<u64>> {
        let token = self.player_token(epoch)?;
        let controller = self.player.controller().await?;
        let before = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        let Some((kind, id)) = selected_resource(&before, play_id)? else {
            return Ok(None);
        };
        let api = self.session.native_api(&token)?;
        let count = tokio::time::timeout(
            Duration::from_secs(5),
            CommentApi::new(api.clone()).count(kind, id.clone()),
        )
        .await
        .map_err(|_| Error::State("player comment count timed out"))??;
        let latest = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        self.session.require_api(&token, &api)?;
        if selected_resource(&latest, play_id)? != Some((kind, id)) {
            return Err(Error::StaleResponse);
        }
        Ok(Some(count))
    }
}
