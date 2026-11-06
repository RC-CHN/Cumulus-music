//! Likes use the visible queue entry, never the currently loaded track.
use super::{MusicClient, PlayerResourceKind, identity};
use crate::{
    Error, Result,
    client::dto::{AccountData, LikeResult},
    protocol::api::identifier,
};
use serde_json::Value;

pub(super) fn song_id(resource: &Value) -> Option<String> {
    let catalog_song = match identity::kind(resource) {
        PlayerResourceKind::Catalog => true,
        PlayerResourceKind::Downloaded => {
            resource["download_task_id"]
                .as_str()
                .is_some_and(|id| !id.is_empty())
                && ["cloud_song_id", "local_id", "voice_id", "download_voice_id"]
                    .into_iter()
                    .all(|key| resource[key].is_null())
        }
        _ => false,
    };
    if !catalog_song || resource["voice"].is_object() || !resource["song"]["voice_id"].is_null() {
        return None;
    }
    let native = identifier(&resource["id"]).ok()?;
    (identifier(&resource["song"]["id"]).ok()? == native).then_some(native)
}

impl MusicClient {
    pub async fn player_queue_like(
        &self,
        epoch: &str,
        revision: &str,
        entry_id: &str,
        liked: bool,
    ) -> Result<AccountData<LikeResult>> {
        let token = self.playlist_token(epoch)?;
        let revision = revision
            .parse::<u64>()
            .map_err(|_| Error::InvalidInput("invalid queue revision"))?;
        let (before, queue) = self
            .player
            .controller()
            .await?
            .queue_with_state(&token)
            .await?;
        self.session.check_generation(&token)?;
        if before.queue_revision != revision {
            return Err(Error::QueueChanged);
        }
        if before.fm || self.queue_clear_context(&token)?.room_id.is_some() {
            return Err(Error::InvalidInput("song like requires the ordinary queue"));
        }
        let id = queue
            .items
            .iter()
            .find(|entry| entry.entry_id == entry_id && entry.visible_in_queue())
            .and_then(|entry| song_id(&entry.resource))
            .ok_or(Error::InvalidInput("queue row has no catalog song"))?;
        self.set_liked(epoch, &id, liked).await
    }
}

#[cfg(test)]
mod tests {
    use super::song_id;
    use serde_json::json;

    #[test]
    fn a_numeric_media_id_cannot_turn_a_foreign_resource_into_a_catalog_song() {
        assert_eq!(
            song_id(&json!({"id":"9007199254740993","song":{"id":"9007199254740993"}})),
            Some("9007199254740993".into())
        );
        for foreign in [
            json!({"id":"14","voice_id":"20","song":{"id":"14"}}),
            json!({"id":"14","song":{"id":"14","voice_id":"20"}}),
            json!({"id":"14","download_task_id":"task","download_voice_id":"20","song":{"id":"14"}}),
            json!({"id":"14","download_task_id":"task","cloud_song_id":"14","song":{"id":"14"}}),
            json!({"id":"14","cloud_song_id":"14","song":{"id":"14"}}),
            json!({"id":"14","local_id":"14","song":{"id":"14"}}),
            json!({"id":"14","song":{"id":"15"}}),
            json!({"id":"14"}),
            json!({"id":"https://example.invalid/song","song":{"id":"14"}}),
        ] {
            assert_eq!(song_id(&foreign), None);
        }
        assert_eq!(
            song_id(
                &json!({"id":"14","download_task_id":"task","download_voice_id":null,
                "cloud_song_id":null,"song":{"id":"14"}})
            ),
            Some("14".into())
        );
    }
}
