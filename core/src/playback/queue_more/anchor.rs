//! Official M/1384 distinguishes the divider's row from recommendation status.
use super::{QueueEntry, QueueSnapshot, active};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Anchor {
    /// A pre-anchor Cumulus record. Different from an intentional no-anchor state.
    #[default]
    Legacy,
    None,
    Entry(String),
}

impl Anchor {
    pub(super) fn is_legacy(&self) -> bool {
        matches!(self, Self::Legacy)
    }
}

fn visible(queue: &QueueSnapshot) -> impl Iterator<Item = &QueueEntry> {
    queue.items.iter().filter(|entry| entry.visible_in_queue())
}

pub(in crate::playback) fn index_and_id(queue: &QueueSnapshot) -> Option<(usize, &str)> {
    match &queue.more.as_ref()?.anchor {
        Anchor::Entry(id) => visible(queue)
            .enumerate()
            .find(|(_, entry)| entry.entry_id == *id)
            .map(|(index, entry)| (index, entry.entry_id.as_str())),
        Anchor::None | Anchor::Legacy => None,
    }
}

fn first_active(queue: &QueueSnapshot) -> Anchor {
    visible(queue)
        .find(|entry| active(queue, entry))
        .map_or(Anchor::None, |entry| Anchor::Entry(entry.entry_id.clone()))
}

fn store(queue: &mut QueueSnapshot, anchor: Anchor) {
    if let Some(more) = &mut queue.more {
        more.anchor = anchor;
    }
}

pub(in crate::playback) fn normalize(queue: &mut QueueSnapshot) {
    match queue.more.as_ref().map(|more| &more.anchor) {
        Some(Anchor::Legacy) => store(queue, first_active(queue)),
        Some(Anchor::Entry(_)) if index_and_id(queue).is_none() => store(queue, Anchor::None),
        _ => {}
    }
}

/// M/1384 restores the file into a newly initialized empty model with index -1.
/// Rebuild its numeric divider from the first active row, independently of
/// a row bit carried over by the disk JSON. Live CAS reloads keep the commit.
pub(in crate::playback) fn restore_cold(queue: &mut QueueSnapshot) {
    store(queue, first_active(queue));
}

pub(in crate::playback) fn reconcile(
    previous: &QueueSnapshot,
    next: &mut QueueSnapshot,
    released_by_move: bool,
) {
    let Some(more) = &next.more else { return };
    let same_origin = previous
        .more
        .as_ref()
        .is_some_and(|old| old.nonce == more.nonce);
    let candidate = match same_origin.then(|| index_and_id(previous)).flatten() {
        None => first_active(next),
        Some((_, id)) if !released_by_move && visible(next).any(|entry| entry.entry_id == id) => {
            Anchor::Entry(id.to_owned())
        }
        Some((old_index, _)) => {
            let new_ids: HashSet<_> = visible(next)
                .skip(if released_by_move { old_index + 1 } else { 0 })
                .map(|entry| entry.entry_id.as_str())
                .collect();
            // M/1384:4874–4900. Deletion can keep any surviving old successor;
            // a drag that released the anchor scans only the new suffix after
            // its old numeric index. An ordinary successor is still eligible.
            let successor = visible(previous)
                .skip(old_index + 1)
                .map(|entry| entry.entry_id.as_str())
                .find(|id| new_ids.contains(id));
            successor.map_or(Anchor::None, |id| Anchor::Entry(id.to_owned()))
        }
    };
    store(next, candidate);
}
