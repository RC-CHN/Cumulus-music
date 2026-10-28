//! M/308 breakpoint selection: a finished/out-of-range episode starts at zero;
//! otherwise use its listen location when the radio's resume setting permits it.
//! Local checkpoints are account scoped; remote history writes are a separate flow.
use crate::{
    Result,
    account::{AccountSession, SessionToken},
    protocol::api::identifier,
    storage::StateTransaction,
};
use serde_json::{Value, json};
const KIND: &str = "podcast-positions-v1";
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
) -> Result<Option<f64>> {
    let Some(id) = resource.get("voice_id").filter(|v| !v.is_null()) else {
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
    let store = session.persistent_store()?;
    let local = session.with_token(token, |owner| store.get(owner, KIND, &id))?;
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
