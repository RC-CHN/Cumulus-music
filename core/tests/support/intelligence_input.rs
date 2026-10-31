use cumulus_core::{
    catalog::intelligence::{IntelligenceRequest, IntelligenceStart},
    playback::{
        PlaybackMode, QueueEntry, QueueSnapshot,
        intelligence::{IntelligenceAction, IntelligenceBegin},
    },
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub fn liked(ids: &[u64]) -> QueueSnapshot {
    let items: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| QueueEntry {
            entry_id: format!("liked-{i}-{id}"),
            resource: json!({"id":id,"dt":120000}),
            source: json!({"kind":"playlist","id":"31"}),
        })
        .collect();
    QueueSnapshot {
        order: items.iter().map(|e| e.entry_id.clone()).collect(),
        current: items.first().map(|e| e.entry_id.clone()),
        items,
        mode: PlaybackMode::OrderLoop,
        ..Default::default()
    }
}
pub fn begin(seed: u64, preserve: bool) -> IntelligenceAction {
    IntelligenceAction::Begin(Box::new(IntelligenceBegin {
        request: IntelligenceRequest {
            playlist_id: "31".into(),
            song_id: seed.to_string(),
            start_music_id: "7".into(),
            count: 3,
            start: IntelligenceStart::PlayOne,
        },
        seed_resource: json!({"id":seed,"dt":120000}),
        seed_source: json!({"kind":"playlist","id":"31"}),
        liked: liked(&[7, 8, 9]),
        preserve_current_queue: preserve,
        liked_metadata: vec![],
    }))
}
pub fn complete(id: &str, ids: &[u64]) -> IntelligenceAction {
    IntelligenceAction::Complete {
        session_id: id.into(),
        resources: ids.iter().map(|id| json!({"id":id,"dt":120000})).collect(),
        recommendations: BTreeMap::new(),
    }
}
pub fn ids(queue: &QueueSnapshot) -> Vec<Value> {
    queue
        .items
        .iter()
        .map(|e| e.resource["id"].clone())
        .collect()
}
