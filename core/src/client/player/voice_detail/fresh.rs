//! M109.j / M15.yb / M21.W: fresh display details, bound to a native voice play.
use super::{
    MusicClient, PlayerResourceKind, PlayerVoiceDetail, bound_episode, cover_url, detail_fields,
    id, identity, radio_fields,
};
use crate::{
    Error, Result,
    client::player::{vinyl_similar::stale_credentials, voice_scope},
    podcast::Radio,
    protocol::{
        api::{VinylVoiceDetailRead, validate_response},
        wire::serialization::js_truthy,
    },
};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VinylVoiceDetailView {
    pub epoch: String,
    pub play_id: String,
    pub entry_id: String,
    pub voice_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub detail: PlayerVoiceDetail,
}

fn same_id(value: &Value, expected: &str) -> bool {
    value.as_str().is_none_or(|value| value.len() <= 128) && id(value).as_deref() == Some(expected)
}

fn match_aliases(candidates: &[&Value], expected: &str) -> bool {
    let mut meaningful = candidates.iter().copied().filter(|value| js_truthy(value));
    meaningful
        .next()
        .is_some_and(|first| same_id(first, expected) && meaningful.all(|v| same_id(v, expected)))
}

fn response_episode(reply: &Value, voice_id: &str, song_id: &str) -> Result<()> {
    if !reply.is_object()
        || !match_aliases(
            &[
                &reply["id"],
                &reply["resourceId"],
                &reply["voiceId"],
                &reply["productId"],
            ],
            voice_id,
        )
        || !match_aliases(
            &[
                &reply["mainTrackId"],
                &reply["mainSong"]["id"],
                &reply["trackId"],
            ],
            song_id,
        )
        || (!reply["programId"].is_null() && !same_id(&reply["programId"], voice_id))
        || (!reply["track"]["id"].is_null() && !same_id(&reply["track"]["id"], song_id))
    {
        return Err(Error::Response("vinyl voice detail identity mismatch"));
    }
    Ok(())
}

fn title(voice: &Value) -> Option<String> {
    let title = if js_truthy(&voice["name"]) {
        &voice["name"]
    } else {
        &voice["voiceName"]
    }
    .as_str()?;
    (title.len() <= 512 * 3
        && !title.trim().is_empty()
        && title.encode_utf16().count() <= 512
        && !title.chars().any(char::is_control))
    .then(|| title.to_owned())
}

impl MusicClient {
    pub async fn player_vinyl_voice_detail(
        &self,
        epoch: &str,
        play_id: &str,
    ) -> Result<VinylVoiceDetailView> {
        let token = self.player_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let snapshot = controller.snapshot();
        let initial = voice_scope::seed(&snapshot, &token, play_id)?;
        let origin = &snapshot
            .selected
            .as_ref()
            .ok_or(Error::StaleResponse)?
            .source;
        let room = self.together_detached_guard(&token);
        let current_scope = || {
            self.session
                .with_verified_profile(&token, &api, |_, _| Ok(()))?;
            let snapshot = controller.snapshot();
            if self.login_pending()?
                || room.is_cancelled()
                || voice_scope::seed(&snapshot, &token, play_id)? != initial
                || snapshot.selected.as_ref().map(|entry| &entry.source) != Some(origin)
            {
                return Err(Error::StaleResponse);
            }
            Ok(snapshot)
        };
        current_scope()?;
        let read = VinylVoiceDetailRead::new(&initial.voice_id)?;
        let revision = api
            .verified_pc_experiment_revision()
            .map_err(stale_credentials)?
            .ok_or(Error::State(
                "vinyl voice detail requires signed native PC credentials",
            ))?;
        let check_authority = || {
            if api
                .verified_pc_experiment_revision()
                .map_err(stale_credentials)?
                .as_deref()
                != Some(revision.as_str())
            {
                return Err(Error::StaleResponse);
            }
            current_scope()
        };
        let serial = &self.player.voice_detail_read;
        let ticket = {
            let mut serial = serial.lock().map_err(|_| Error::LockPoisoned)?;
            check_authority()?;
            *serial = serial
                .checked_add(1)
                .ok_or(Error::State("vinyl voice detail reads exhausted"))?;
            *serial
        };
        let current = || {
            let snapshot = check_authority()?;
            if *serial.lock().map_err(|_| Error::LockPoisoned)? != ticket {
                return Err(Error::StaleResponse);
            }
            Ok(snapshot)
        };
        let permission = || current().map(|_| ());
        permission()?;
        let reply = api
            .request_vinyl_voice_detail(read, &revision, &permission)
            .await;
        permission()?;
        let reply = validate_response(
            VinylVoiceDetailRead::PATH,
            reply.map_err(stale_credentials)?,
            &[200],
        )?;
        let voice = &reply["program"];
        response_episode(voice, &initial.voice_id, &initial.song_id)?;
        let owner = token.uid.as_deref();
        let radio = Radio::from_official(&voice["radio"])
            .ok()
            .and_then(|radio| {
                let normalized = radio.normalized();
                radio_fields(voice, owner, id(&normalized["id"])?, &normalized)
            });
        let title = title(voice);
        // VoiceCover reads the player's artwork independently of the fresh
        // program. It cannot be replaced by artwork from this detail response.
        let snapshot = current()?;
        let resource = &snapshot
            .selected
            .as_ref()
            .ok_or(Error::StaleResponse)?
            .resource;
        let (_, _, bound) = bound_episode(resource).ok_or(Error::StaleResponse)?;
        let cover = cover_url(
            bound,
            identity::kind(resource) == PlayerResourceKind::Downloaded,
        );
        let detail = detail_fields(voice, initial.voice_id.clone(), cover, radio);
        permission()?;
        Ok(VinylVoiceDetailView {
            epoch: token.generation.to_string(),
            play_id: play_id.to_owned(),
            entry_id: initial.entry_id.clone(),
            voice_id: initial.voice_id.clone(),
            title,
            detail,
        })
    }
}
