//! Optional song progress annotation, independent of source authorization.
use super::{MusicClient, PlaybackCommand, PlaybackSnapshot, PlayerResourceKind, identity};
use crate::{Error, Result, playback::TimelineMode, protocol::api::identifier};
use serde_json::{Value, json};
use std::time::Duration;

fn selected_song<'a>(snapshot: &'a PlaybackSnapshot, play_id: &str) -> Result<Option<&'a Value>> {
    if snapshot.play_id.as_deref() != Some(play_id) {
        return Err(Error::StaleResponse);
    }
    if !snapshot
        .timeline
        .as_ref()
        .is_some_and(|timeline| timeline.mode != TimelineMode::Cue && timeline.end.is_some())
    {
        return Ok(None);
    }
    Ok(snapshot.selected.as_ref().and_then(|entry| {
        if identity::kind(&entry.resource) != PlayerResourceKind::Catalog {
            return None;
        }
        let id = &entry.resource["id"];
        identifier(id).ok().map(|_| id)
    }))
}

fn start_seconds(response: &Value, song: &str) -> Option<f64> {
    response["data"].as_array()?.iter().find_map(|row| {
        if identifier(&row["id"]).ok().as_deref() != Some(song) {
            return None;
        }
        let start = row["startTime"].as_f64()? / 1000.0;
        (start.is_finite() && start > 0.0).then_some(start)
    })
}

impl MusicClient {
    pub async fn player_chorus(&self, epoch: &str, play_id: &str) -> Result<Option<f64>> {
        let token = self.player_token(epoch)?;
        let controller = self.player.controller().await?;
        let current = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        let Some(song) = selected_song(&current, play_id)? else {
            return Ok(None);
        };
        let ids = serde_json::to_string(&[song])?;
        // This optional decoration must not expire an otherwise working
        // playback session on a failed metadata read.
        let api = self.session.native_api(&token)?;
        let response = tokio::time::timeout(
            Duration::from_secs(5),
            api.call("/api/song/chorus", json!({ "ids": ids }), false, &[200]),
        )
        .await
        .map_err(|_| Error::State("song chorus request timed out"))??;
        let latest = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        self.session.require_api(&token, &api)?;
        if selected_song(&latest, play_id)? != Some(song) {
            return Err(Error::StaleResponse);
        }
        Ok(start_seconds(&response, &identifier(song)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chorus_requires_positive_milliseconds_for_the_requested_song() {
        let data = json!({ "data": [
            { "id": 11, "startTime": 4321 },
            { "id": 10, "startTime": 0 },
            { "id": 10, "startTime": "1234" },
            { "id": "10", "startTime": 1234 }
        ] });
        assert_eq!(start_seconds(&data, "10"), Some(1.234));
        assert_eq!(start_seconds(&data, "12"), None);
        assert_eq!(
            start_seconds(&json!({ "data": { "10": 1000 } }), "10"),
            None
        );
    }
}
