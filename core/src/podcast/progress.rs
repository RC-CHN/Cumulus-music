//! M/308 breakpoint selection: a finished/out-of-range episode starts at zero;
//! otherwise use its listen location when the radio's resume setting permits it.
//! Local checkpoints are account scoped; remote history writes are a separate flow.
pub(crate) mod latest;

use crate::{
    Result,
    account::{AccountSession, SessionToken},
    protocol::api::identifier,
    storage::StateTransaction,
};
use serde_json::{Value, json};
const KIND: &str = "podcast-positions-v1";
pub(crate) const PLAYRECORD_PATH: &str = "/api/dj/playrecord/upload";

/// Episode identity is distinct from the ID of the playable main song.
/// Downloaded episodes can carry the same native metadata inside `song`.
fn program_identity(resource: &Value) -> Option<(&Value, String, String)> {
    let voice = resource
        .get("voice")
        .filter(|voice| voice.is_object())
        .or_else(|| resource["song"].get("voice"))?;
    let id = identifier(&voice["id"]).ok()?;
    let mut claimed = [&resource["voice_id"], &resource["song"]["voice_id"]]
        .into_iter()
        .filter(|value| !value.is_null());
    if identifier(claimed.next()?).ok()? != id
        || claimed.any(|value| identifier(value).ok().as_ref() != Some(&id))
    {
        return None;
    }
    let main_song = voice
        .get("mainTrackId")
        .filter(|value| !value.is_null())
        .unwrap_or(&voice["mainSong"]["id"]);
    let media_id = identifier(&resource["id"]).ok()?;
    (identifier(main_song).ok()? == media_id).then_some((voice, id, media_id))
}

pub(crate) fn program_id(resource: &Value) -> Option<u64> {
    let (_, id, _) = program_identity(resource)?;
    let number = id.parse::<u64>().ok()?;
    (number > 0 && number < (1_u64 << 53)).then_some(number)
}

/// M/308 passes the last stored progress for an interruption; native M/110
/// stores whole seconds for ordinary playback. EOF/skip-outro use the full
/// duration. M/71 calculates the wire completion flag independently.
pub(crate) fn playrecord(
    program_id: u64,
    position: f64,
    duration: f64,
    completed: bool,
) -> Option<Value> {
    if !position.is_finite() || position < 0.0 || !duration.is_finite() || duration <= 0.0 {
        return None;
    }
    let seconds = if completed {
        duration
    } else {
        position.floor()
    };
    let millis = 1000.0 * seconds;
    if !millis.is_finite() {
        return None;
    }
    Some(json!({
        "programId": program_id,
        "listenLocation": millis,
        "isListened": seconds.floor() >= duration.floor(),
    }))
}
/// Local listening history includes previews, without treating them as a
/// resumable full program or an entitlement to the remaining content.
pub(crate) fn mark_heard(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    resource: &Value,
) -> Result<()> {
    if let Some(voice) = resource
        .get("voice_id")
        .filter(|v| !v.is_null())
        .or_else(|| resource["song"].get("voice_id").filter(|v| !v.is_null()))
    {
        let id = identifier(voice)?;
        if tx.get(owner, "podcast-heard-v1", &id)?.is_none() {
            tx.put(owner, "podcast-heard-v1", &id, &json!(true))?;
        }
    }
    Ok(())
}
pub(crate) fn save(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    resource: &Value,
    seconds: f64,
    ended: bool,
) -> Result<()> {
    let voice = resource
        .get("voice_id")
        .or_else(|| resource["song"].get("voice_id"));
    let Some(voice) = voice.filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let id = identifier(voice)?;
    let media = identifier(&resource["id"])?;
    tx.put(
        owner,
        KIND,
        &id,
        &json!({"voiceId":id,"mediaId":media,"listenLocation":seconds*1000.0,"isListened":ended}),
    )
}
pub(crate) async fn position(
    session: &AccountSession,
    token: &SessionToken,
    resource: &Value,
    supplied_checkpoint: bool,
) -> Result<Option<f64>> {
    let Some(id) = resource.get("voice_id").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let id = identifier(id)?;
    let media = identifier(&resource["id"])?;
    let radio = &resource["voice"]["audio"]["id"];
    if radio.is_null() {
        return Ok(None);
    }
    // Re-evaluate on selection, never cache a checkpoint together with a URL.
    let enabled = session
        .run_for(token, |api, _| async {
            super::PodcastApi::new(api).resume_enabled(radio).await
        })
        .await?;
    if !enabled {
        return Ok(None);
    }
    record_position_for(session, token, resource, supplied_checkpoint, &id, &media)
}

pub(crate) fn record_position(
    session: &AccountSession,
    token: &SessionToken,
    resource: &Value,
    supplied_checkpoint: bool,
) -> Result<Option<f64>> {
    let id = identifier(&resource["voice_id"])?;
    let media = identifier(&resource["id"])?;
    record_position_for(session, token, resource, supplied_checkpoint, &id, &media)
}

fn record_position_for(
    session: &AccountSession,
    token: &SessionToken,
    resource: &Value,
    supplied_checkpoint: bool,
    id: &str,
    media: &str,
) -> Result<Option<f64>> {
    let local = if supplied_checkpoint {
        // The official listened-book action supplies its displayed checkpoint
        // for this selection. Do not substitute another generic local clock.
        None
    } else {
        let store = session.persistent_store()?;
        session.with_token(token, |owner| store.get(owner, KIND, id))?
    };
    let record = local
        .as_ref()
        .filter(|r| r["voiceId"] == id && r["mediaId"] == media)
        .unwrap_or(&resource["voice"]["djPlayRecordVo"]);
    let location = record["listenLocation"].as_f64().unwrap_or(0.0);
    let duration = resource["voice"]["duration"].as_f64().unwrap_or(0.0);
    let seconds = if record["isListened"] == true
        || !location.is_finite()
        || location < 0.0
        || (duration > 0.0 && location >= duration)
    {
        0.0
    } else {
        location / 1000.0
    };
    session.with_token(token, |_| Ok(Some(seconds)))
}
