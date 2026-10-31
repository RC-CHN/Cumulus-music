use super::*;

pub(super) fn restore_current(
    base: &QueueSnapshot,
    current: &QueueSnapshot,
    keep_missing: bool,
) -> QueueSnapshot {
    let mut queue = base.clone();
    let Some(selected) = current.selected() else {
        queue.current = queue.items.first().map(|e| e.entry_id.clone());
        return queue;
    };
    let song_id = resource_id(&selected.resource).ok();
    if let Some(index) = queue
        .items
        .iter()
        .position(|e| song_id.is_some() && resource_id(&e.resource).ok() == song_id)
    {
        let old = queue.items[index].entry_id.clone();
        let mut restored = selected.clone();
        restored.source = queue.items[index].source.clone();
        queue.items[index] = restored;
        queue
            .order
            .iter_mut()
            .filter(|id| **id == old)
            .for_each(|id| *id = selected.entry_id.clone());
        queue.current = Some(selected.entry_id);
    } else if keep_missing {
        let before = current
            .items
            .iter()
            .position(|e| e.entry_id == selected.entry_id)
            .unwrap_or(0);
        let index = current.items[..before]
            .iter()
            .rev()
            .find_map(|e| {
                let id = resource_id(&e.resource).ok()?;
                queue
                    .items
                    .iter()
                    .position(|e| resource_id(&e.resource).ok().as_ref() == Some(&id))
                    .map(|i| i + 1)
            })
            .unwrap_or(0);
        let mut selected = selected;
        if let Some(source) = selected.source.as_object_mut() {
            source.remove(TRANSIENT);
            source.insert(RETAINED.into(), json!(true));
        }
        queue.current = Some(selected.entry_id.clone());
        queue.order.insert(index, selected.entry_id.clone());
        queue.items.insert(index, selected);
    } else {
        queue.current = queue.items.first().map(|e| e.entry_id.clone());
    }
    queue
}
