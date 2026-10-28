use super::super::{PublicationEvent, PublicationFrame};
use crate::relay::RelayCommand;
use std::collections::{HashMap, HashSet};

/// O(n log n) longest increasing subsequence identifies retained rows. Moving
/// each other retained row after its final predecessor produces the requested
/// order without an O(n²) simulation of a large playlist.
fn kept(indices: &[usize]) -> HashSet<usize> {
    let mut tails: Vec<usize> = vec![];
    let mut links = vec![None; indices.len()];
    for (i, &value) in indices.iter().enumerate() {
        let slot = tails.partition_point(|&j| indices[j] < value);
        if slot > 0 {
            links[i] = Some(tails[slot - 1]);
        }
        if slot == tails.len() {
            tails.push(i)
        } else {
            tails[slot] = i
        }
    }
    let mut result = HashSet::new();
    let mut next = tails.last().copied();
    while let Some(i) = next {
        result.insert(i);
        next = links[i];
    }
    result
}
pub fn events(
    before: &PublicationFrame,
    after: &PublicationFrame,
    replace: bool,
    from_handoff: bool,
) -> Vec<PublicationEvent> {
    if !after.can_publish() {
        return if before.can_publish() {
            vec![PublicationEvent::Clear]
        } else {
            vec![]
        };
    }
    if replace || !before.can_publish() {
        return vec![PublicationEvent::Source];
    }
    let old: HashMap<_, _> = before
        .items
        .iter()
        .enumerate()
        .map(|(n, i)| (i.entry_id.as_str(), (n, i)))
        .collect();
    let new: HashSet<_> = after.items.iter().map(|i| i.entry_id.as_str()).collect();
    if after
        .items
        .iter()
        .all(|i| !old.contains_key(i.entry_id.as_str()))
    {
        return vec![PublicationEvent::Source];
    }
    if after.items.iter().any(|i| {
        old.get(i.entry_id.as_str())
            .is_some_and(|(_, old)| old.resource != i.resource)
    }) {
        return vec![PublicationEvent::Snapshot];
    }
    let changed = before.items != after.items;
    if changed {
        // preId contains no type or entry identity. Duplicate resource IDs make
        // incremental insertion/movement ambiguous; preserve the full ordering.
        for items in [&before.items, &after.items] {
            let mut ids = HashSet::new();
            if items.iter().any(|i| !ids.insert(&i.resource.id)) {
                return vec![PublicationEvent::Snapshot];
            }
        }
    }
    let mut events = vec![];
    let deleted: Vec<_> = before
        .items
        .iter()
        .filter(|i| !new.contains(i.entry_id.as_str()))
        .map(|i| i.resource.clone())
        .collect();
    if !deleted.is_empty() {
        events.push(PublicationEvent::Command {
            command: RelayCommand::Delete { resources: deleted },
        });
    }
    let retained: Vec<_> = after
        .items
        .iter()
        .filter(|i| old.contains_key(i.entry_id.as_str()))
        .collect();
    let indices: Vec<_> = retained
        .iter()
        .map(|i| old[i.entry_id.as_str()].0)
        .collect();
    let kept = kept(&indices);
    for (i, item) in retained.iter().enumerate() {
        if !kept.contains(&i) {
            events.push(PublicationEvent::Command {
                command: RelayCommand::Move {
                    resources: vec![item.resource.clone()],
                    pre_id: i.checked_sub(1).map(|n| retained[n].resource.id.clone()),
                },
            });
        }
    }
    let mut index = 0;
    while index < after.items.len() {
        if old.contains_key(after.items[index].entry_id.as_str()) {
            index += 1;
            continue;
        }
        let begin = index;
        while index < after.items.len() && !old.contains_key(after.items[index].entry_id.as_str()) {
            index += 1;
        }
        events.push(PublicationEvent::Command {
            command: RelayCommand::Insert {
                resources: after.items[begin..index]
                    .iter()
                    .map(|i| i.resource.clone())
                    .collect(),
                pre_id: begin
                    .checked_sub(1)
                    .map(|i| after.items[i].resource.id.clone()),
            },
        });
    }
    if events.len() > 512 {
        return vec![PublicationEvent::Snapshot];
    }
    if before.selected_entry != after.selected_entry || before.current != after.current {
        events.push(PublicationEvent::Select);
    }
    if before.mode != after.mode && !from_handoff {
        events.push(PublicationEvent::Mode {
            from_handoff: false,
        });
    }
    events
}
