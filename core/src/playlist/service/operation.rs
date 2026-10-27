use super::*;
use serde_json::json;
use std::collections::{BTreeMap, HashSet};
pub(super) struct Ack {
    pub created: Option<String>,
    pub codes: BTreeMap<String, i64>,
}
pub(super) fn permutation(before: &[String], desired: &[String]) -> Result<()> {
    if before.len() != desired.len()
        || before.iter().collect::<HashSet<_>>() != desired.iter().collect()
    {
        return Err(Error::InvalidInput(
            "reorder must retain every id exactly once",
        ));
    }
    Ok(())
}
pub(super) async fn send(
    api: PlaylistApi,
    uid: &str,
    command: PlaylistCommand,
    before: Value,
) -> Result<Ack> {
    let mut created = None;
    let batch = matches!(command, PlaylistCommand::Metadata { .. });
    match command {
        PlaylistCommand::Create { name, private } => {
            created = Some(api.create(&json!(uid), &name, private).await?);
        }
        PlaylistCommand::Metadata {
            id,
            name,
            tags,
            description,
            ..
        } => {
            api.update_metadata(&json!(id), &name, &tags, &description)
                .await?;
        }
        PlaylistCommand::Tracks {
            id, change, ids, ..
        } => {
            let ids = if change == TrackChange::Clear {
                serde_json::from_value::<PlaylistSnapshot>(before)?.tracks
            } else {
                ids
            };
            api.change_tracks(
                &json!(id),
                &ids.into_iter().map(Value::String).collect::<Vec<_>>(),
                match change {
                    TrackChange::Add => TrackOperation::Add,
                    TrackChange::Delete | TrackChange::Clear => TrackOperation::Delete,
                    _ => TrackOperation::Replace,
                },
            )
            .await?;
        }
        PlaylistCommand::Delete { id, .. } => {
            api.delete(&json!(id)).await?;
        }
        PlaylistCommand::Cover {
            id, document_id, ..
        } => {
            api.cover(&json!(id), &json!(document_id)).await?;
        }
        PlaylistCommand::Publish { id, .. } => {
            api.publish(&json!(id)).await?;
        }
        PlaylistCommand::Subscribe { id, subscribed } => {
            api.subscribe(&json!(id), subscribed).await?;
        }
        PlaylistCommand::Order { ids, .. } => {
            api.order(&ids.into_iter().map(Value::String).collect::<Vec<_>>())
                .await?;
        }
    }
    Ok(Ack {
        created,
        codes: if batch {
            ["name", "tags", "description"]
                .into_iter()
                .map(|s| (s.into(), 200))
                .collect()
        } else {
            BTreeMap::from([("main".into(), 200)])
        },
    })
}
fn code(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str()?.parse().ok())
}
pub(super) fn error_codes(error: &Error) -> BTreeMap<String, i64> {
    match error.root() {
        Error::Batch { responses, .. } => [
            ("name", "/api/playlist/update/name"),
            ("tags", "/api/playlist/tags/update"),
            ("description", "/api/playlist/desc/update"),
        ]
        .into_iter()
        .filter_map(|(field, path)| Some((field.into(), code(&responses[path]["code"])?)))
        .collect(),
        Error::Service { code: v, .. } => code(v)
            .map(|n| BTreeMap::from([("main".into(), n)]))
            .unwrap_or_default(),
        Error::HttpStatus {
            body: Some(body), ..
        } => code(&body["code"])
            .map(|n| BTreeMap::from([("main".into(), n)]))
            .unwrap_or_default(),
        _ => BTreeMap::new(),
    }
}
pub(super) fn unmatched_phase(record: &PlaylistMutation) -> PlaylistPhase {
    let rejected = |n: i64| [400, 403, 405, 406, 502, 505, 506, 511, 512].contains(&n);
    if record.codes.get("main").is_some_and(|n| rejected(*n)) {
        return PlaylistPhase::Rejected;
    }
    if matches!(record.command, PlaylistCommand::Metadata { .. })
        && ["name", "tags", "description"].iter().all(|f| {
            record
                .codes
                .get(*f)
                .is_some_and(|n| *n == 200 || rejected(*n))
        })
        && record.codes.values().any(|n| rejected(*n))
    {
        return if record.codes.values().any(|n| *n == 200) {
            PlaylistPhase::Partial
        } else {
            PlaylistPhase::Rejected
        };
    }
    PlaylistPhase::Uncertain
}
