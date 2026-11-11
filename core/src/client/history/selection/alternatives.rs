//! M59's explicit type-1 same recording lookup. This does not authorize audio.
use crate::{
    Result,
    catalog::{merge_song_privileges, same_recording_details},
    client::mapping,
    membership::Quality,
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(super) fn declared(
    original: &str,
    song: Option<&Value>,
    privilege: Option<&Value>,
) -> Option<String> {
    let (song, privilege) = (song?, privilege?);
    if !privilege.is_object() || mapping::online_play_all_eligible(&json!({"privilege": privilege}))
    {
        return None;
    }
    let candidate = crate::playback::intelligence::eligibility::identical_recording(song)?;
    (candidate != original && candidate.len() <= 32).then_some(candidate)
}

pub(super) async fn resolve(
    api: Arc<dyn Api>,
    ids: &[String],
    quality: Quality,
) -> Result<BTreeMap<String, Value>> {
    let requested: BTreeSet<_> = ids.iter().map(String::as_str).collect();
    let mut result = BTreeMap::new();
    for batch in same_recording_details(api, ids).await? {
        // M21.B attaches the reply privilege for each returned ID. Unknown
        // rights and metadata-only stubs cannot supply a known playable
        // replacement here. The ordinary resolver still checks the URL.
        for row in merge_song_privileges(&batch.songs, &batch.privileges)? {
            let id = identifier(&row["id"])?;
            if !requested.contains(id.as_str())
                || row["name"].as_str().is_none_or(str::is_empty)
                || !row["privilege"].is_object()
                || !mapping::online_play_all_eligible(&row)
            {
                continue;
            }
            let metadata = mapping::song_metadata(&row)?;
            let dt = metadata["dt"].clone();
            let resource = json!({
                "id":id, "level":quality.as_str(), "dt":dt, "song":metadata,
                "privilege":row["privilege"], "trial_mode":-1,
            });
            result.insert(id, resource);
        }
    }
    Ok(result)
}
