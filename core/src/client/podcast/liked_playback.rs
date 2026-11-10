//! Keep withdrawn cards in the liked page, outside its ordinary playback queue.
use super::{MusicClient, PodcastApi, SessionToken};
use crate::{Error, Result, podcast::Episode, protocol::api::Api};
use serde_json::{Value, json};
use std::sync::Arc;

impl MusicClient {
    pub(super) async fn liked_podcast_resources(
        &self,
        token: &SessionToken,
        voices: Vec<Episode>,
        start_id: Option<&str>,
    ) -> Result<(Vec<Value>, usize)> {
        let api = self.session.native_api(token)?;
        // Resolve an explicit gesture first: siblings must never change its
        // meaning or lead to another programme starting when this one fails.
        let selected = if let Some(id) = start_id {
            let row = voices
                .iter()
                .find(|voice| voice.id == id)
                .ok_or(Error::QueueChanged)?;
            if row.display_status_withdrawn() {
                return Err(Error::State("liked voice withdrawn"));
            }
            Some(
                self.liked_voice_media(token, &api, row)
                    .await?
                    .ok_or(Error::State("liked voice media unavailable"))?,
            )
        } else {
            None
        };
        let mut resources = Vec::with_capacity(voices.len());
        let mut index = 0;
        for row in &voices {
            if Some(row.id.as_str()) == start_id {
                index = resources.len();
                resources.push(selected.as_ref().ok_or(Error::QueueChanged)?.clone());
            } else if let Some(resource) = self.liked_voice_media(token, &api, row).await? {
                resources.push(resource);
            }
        }
        self.session.require_api(token, &api)?;
        if resources.is_empty() {
            return Err(Error::State("liked voice queue has no playable entries"));
        }
        Ok((resources, index))
    }

    async fn liked_voice_media(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        card: &Episode,
    ) -> Result<Option<Value>> {
        self.session.require_api(token, api)?;
        if card.display_status_withdrawn() || card.raw["grayDisplay"] == true {
            return Ok(None);
        }
        let detail;
        let voice = if card.song_id.is_none() {
            let reply = self
                .session
                .run_for(token, |current, _| async move {
                    if !Arc::ptr_eq(api, &current) {
                        return Err(Error::StaleResponse);
                    }
                    PodcastApi::new(current).episode(&json!(card.id)).await
                })
                .await;
            detail = match reply {
                Ok(episode) => {
                    self.session.require_api(token, api)?;
                    episode
                }
                Err(error)
                    if error.is_service_code(404)
                        || matches!(error.root(), Error::HttpStatus { status: 404, .. }) =>
                {
                    self.session.require_api(token, api)?;
                    return Ok(None);
                }
                Err(error) => return Err(error),
            };
            if let (Some(original), Some(observed)) = (&card.radio, &detail.radio)
                && original.id != observed.id
            {
                return Err(Error::Response("liked voice detail radio mismatch"));
            }
            &detail
        } else {
            card
        };
        if voice.song_id.is_none()
            || voice.display_status_withdrawn()
            || voice.raw["grayDisplay"] == true
        {
            return Ok(None);
        }
        let mut resource = voice.playback_resource()?;
        resource["song"] = voice.download_metadata()?;
        resource["level"] = json!("standard");
        Ok(Some(resource))
    }
}
