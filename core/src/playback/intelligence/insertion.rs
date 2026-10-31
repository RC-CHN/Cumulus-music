use super::*;
use std::collections::{HashMap, HashSet};

// M/308 play resolves offset=-1 to the current displayOrder, M/59 deduplicates
// playable rows by first song ID, then M/1384 inserts them after that offset.
pub(super) fn append(
    current: &QueueSnapshot,
    session: &IntelligenceSession,
    resources: Vec<Value>,
) -> Result<QueueSnapshot> {
    if resources.len() > MAX_LIKED_TRACKS {
        return Err(Error::InvalidInput("too many intelligence tracks"));
    }
    let mut seen = HashSet::new();
    let resources = resources
        .into_iter()
        .filter_map(|resource| match resource_id(&resource) {
            Ok(id) if seen.insert(id.clone()) => Some(Ok(resource)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .take(MAX_QUEUE_TRACKS)
        .collect::<Result<Vec<_>>>()?;
    let new_ids: HashSet<_> = resources.iter().map(resource_id).collect::<Result<_>>()?;
    let selected = current.selected();
    let offset = current
        .items
        .iter()
        .position(|e| Some(&e.entry_id) == current.current.as_ref())
        .map_or(0, |i| i + 1);
    let available = MAX_QUEUE_TRACKS - resources.len();
    let mut removed = HashSet::new();
    let mut prefix = Vec::new();
    for entry in current.items[..offset].iter().rev() {
        let id = resource_id(&entry.resource)?;
        if prefix.len() >= available || new_ids.contains(&id) {
            removed.insert(id);
        } else {
            prefix.push(entry);
        }
    }
    prefix.reverse();
    let mut suffix = Vec::new();
    for entry in &current.items[offset..] {
        let id = resource_id(&entry.resource)?;
        if prefix.len() + suffix.len() >= available || new_ids.contains(&id) {
            removed.insert(id);
        } else {
            suffix.push(entry);
        }
    }
    let mut slots = HashMap::new();
    for (i, e) in prefix.iter().enumerate() {
        slots.insert(resource_id(&e.resource)?, i);
    }
    for (i, e) in suffix.iter().enumerate() {
        slots.insert(
            resource_id(&e.resource)?,
            prefix.len() + resources.len() + i,
        );
    }
    let mut ordered = current
        .items
        .iter()
        .filter_map(|entry| match resource_id(&entry.resource) {
            Ok(id) if !removed.contains(&id) => Some(Ok((slots[&id], entry.clone()))),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>>>()?;
    for (i, resource) in resources.into_iter().enumerate() {
        let id = resource_id(&resource)?;
        let entry = selected.as_ref().filter(|e| resource_id(&e.resource).ok().as_deref() == Some(&id)).cloned().unwrap_or_else(|| QueueEntry {
            entry_id: Uuid::new_v4().simple().to_string(), resource,
                source: json!({"kind":"playlist","id":session.request.playlist_id,"scene":"aiMode"}),
        });
        ordered.push((prefix.len() + i, entry));
    }
    ordered.sort_by_key(|(slot, _)| *slot);
    let mut items: Vec<_> = ordered.into_iter().map(|(_, e)| e).collect();
    if let Some(mut selected) = selected
        && !items.iter().any(|e| e.entry_id == selected.entry_id)
    {
        let source = selected
            .source
            .as_object_mut()
            .ok_or(Error::InvalidInput("invalid intelligence source"))?;
        source.insert(TRANSIENT.into(), json!(true));
        items.insert(0, selected);
    }
    let mut queue = current.clone();
    queue.current = current
        .current
        .clone()
        .filter(|id| items.iter().any(|e| &e.entry_id == id))
        .or_else(|| items.first().map(|e| e.entry_id.clone()));
    queue.order = items.iter().map(|e| e.entry_id.clone()).collect();
    queue.items = items;
    Ok(queue)
}
