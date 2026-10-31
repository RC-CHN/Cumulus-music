use super::super::PublicationFrame;
use crate::{
    Result,
    playback::QueueSnapshot,
    relay::{RelayResource, RelayResourceKind, RelaySource, RelaySourceKind, SnapshotItem},
};
use std::collections::HashSet;

fn excluded(source: &serde_json::Value) -> bool {
    matches!(
        source["kind"].as_str(),
        Some("local" | "cloud" | "downloads" | "download" | "fm" | "scene_fm")
    ) || source["scene"]
        .as_str()
        .is_some_and(RelaySource::excluded_scene)
}
/// Projection follows the physical list, not randomized traversal order.
/// A continued queue keeps its original collection despite changing the first
/// row by deletion/reorder. Metadata, paths and authorization never enter it.
pub fn project(
    queue: &QueueSnapshot,
    previous: Option<&PublicationFrame>,
    replace: bool,
) -> Result<PublicationFrame> {
    queue.validate()?;
    let mut frame = PublicationFrame {
        queue_revision: 0,
        items: vec![],
        selected_entry: None,
        current: None,
        source: None,
        mode: Some(queue.mode.into()),
        eligible_scene: false,
    };
    if queue
        .intelligence
        .as_ref()
        .is_some_and(crate::playback::intelligence::IntelligenceContext::active)
        || queue
            .items
            .iter()
            .any(|e| e.source[crate::playback::intelligence::TRANSIENT] == true)
    {
        return Ok(frame);
    }
    let Some(first) = queue.items.first() else {
        return Ok(frame);
    };
    let old_ids: HashSet<_> = previous
        .into_iter()
        .flat_map(|p| p.items.iter().map(|i| i.entry_id.as_str()))
        .collect();
    let continued = !replace
        && queue
            .items
            .iter()
            .any(|i| old_ids.contains(i.entry_id.as_str()));
    if (!continued && excluded(&first.source))
        || (continued && previous.is_some_and(|p| !p.eligible_scene))
    {
        return Ok(frame);
    }
    for item in &queue.items {
        // Files without a canonical service identity cannot be represented by
        // this protocol. Do not drop them and pretend the remote queue matches.
        if item.resource.get("path").is_some() || item.resource.get("local_id").is_some() {
            return Ok(PublicationFrame {
                items: vec![],
                ..frame
            });
        }
        let resource = if let Some(id) = item.resource.get("voice_id") {
            RelayResource::new(id, RelayResourceKind::Voice)?
        } else {
            RelayResource::new(&item.resource["id"], RelayResourceKind::Song)?
        };
        frame.items.push(SnapshotItem {
            entry_id: item.entry_id.clone(),
            resource,
        });
    }
    frame.selected_entry = queue.current.clone();
    frame.current = frame
        .items
        .iter()
        .find(|i| Some(&i.entry_id) == queue.current.as_ref())
        .map(|i| i.resource.clone());
    let collection = if continued {
        previous
            .and_then(|p| p.source.as_ref())
            .filter(|s| matches!(s, RelaySource::Collection { .. }))
            .cloned()
    } else {
        let kind = first.source["kind"].as_str();
        let (kind, id) = match kind {
            Some("playlist") => (Some(RelaySourceKind::Playlist), &first.source["id"]),
            Some("album") => (Some(RelaySourceKind::Album), &first.source["id"]),
            Some("podcast") if first.source["scope"]["kind"] == "radio" => {
                (Some(RelaySourceKind::Podcast), &first.source["scope"]["id"])
            }
            Some("podcast") if !first.source["id"].is_null() => {
                (Some(RelaySourceKind::Podcast), &first.source["id"])
            }
            _ => (None, &serde_json::Value::Null),
        };
        kind.map(|source_type| {
            Ok::<_, crate::Error>(RelaySource::Collection {
                source_type,
                id: crate::protocol::api::identifier(id)?,
            })
        })
        .transpose()?
    };
    frame.source = Some(collection.unwrap_or_else(|| RelaySource::Resources {
        resources: frame.items.iter().map(|i| i.resource.clone()).collect(),
    }));
    frame.eligible_scene = true;
    frame.validate()?;
    Ok(frame)
}
