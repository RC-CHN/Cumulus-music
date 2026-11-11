//! M3085 history queue gestures share the guarded ordinary queue commit.
mod play_all;
mod shuffle;
#[cfg(test)]
mod tests;
use super::{MusicClient, PlayerResourceKind, PlayerView, identity, mapping, project};
use crate::{
    Error, Result,
    account::SessionToken,
    client::history::{Selection, SelectionRows},
    history::HistoryKind,
    playback::{
        PlaybackMode, QueueSnapshot,
        controller::{HistoryQueueUpdate, PlaybackCommand, PlaybackStatus},
        engine::Cancellation,
    },
    podcast::progress::latest::ShuffleAnchor,
};
use rand::{Rng, seq::IndexedRandom};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

impl MusicClient {
    pub(super) async fn history_batch(
        &self,
        token: &SessionToken,
        selection: Selection<'_>,
        prepared: &super::prepared_selection::PreparedSelection,
        play: bool,
    ) -> Result<PlayerView> {
        let room = self.together_detached_guard(token);
        room.check()?;
        self.validate_queue_clear_room(token, None)?;
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(token).await?;
        prepared.check(self, &before)?;
        let kind = selection.kind;
        let full_list = matches!(selection.rows, SelectionRows::All);
        let row_next = matches!(selection.rows, SelectionRows::RowNext { .. });
        let play_all = full_list && play;
        if !play_all && (before.fm || queue.intelligence.is_some()) {
            return Err(Error::State("history batch requires an ordinary queue"));
        }
        if matches!(
            before.state,
            PlaybackStatus::Preparing | PlaybackStatus::Loading
        ) {
            return Err(Error::MutationBusy);
        }
        if row_next
            && (queue.items.iter().any(|entry| !entry.visible_in_queue())
                || queue.mode == PlaybackMode::Random
                    && !queue.items.is_empty()
                    && queue.current.is_none())
        {
            // M1384's Random queue-only branch needs a current entry. Never
            // turn this nonempty queue into a start or silently replace it.
            return Err(Error::State("history row next requires an ordinary queue"));
        }
        let (replacement_mode, fm_backup_queue) = if play_all && before.fm {
            let store = self.session.persistent_store()?;
            let backup = self.session.with_local_owner(token, |owner| {
                store.get(owner, "playback", crate::playback::FM_BACKUP_QUEUE)
            })?;
            (play_all::fm_return_mode(backup.as_ref())?, backup)
        } else {
            (before.intelligence_last_mode.unwrap_or(queue.mode), None)
        };
        let revision = selection.revision.to_owned();
        let (resources, index, source) = self
            .history_selection_for_queue(token, selection, prepared.observed, play)
            .await?;
        let pending = self.pending_preparation_guard(prepared, controller.clone());
        room.check()?;
        pending.check()?;
        let was_empty = queue.items.is_empty();
        let start = was_empty || play;
        // Selected Add carries the first selected voice as its resource.
        // Header Add All has resource {}, and M3085 instead takes the first
        // listData ID as its explicit start without a radio bookmark lookup.
        // Header Play All suppresses the default list-head playId, but its
        // real first voice resource can still supply a same-episode bookmark.
        let shuffle_anchor = if kind == HistoryKind::Voice
            && (play_all
                || was_empty
                    && (row_next || !play && !full_list && queue.mode == PlaybackMode::Random))
        {
            match resources.get(if row_next { index } else { 0 }) {
                Some(first) => {
                    self.history_shuffle_anchor(token, first, &pending, &room)
                        .await?
                }
                None => None,
            }
        } else {
            None
        };
        pending.check()?;
        let anchor = shuffle_anchor.as_ref().map(|binding| &binding.anchor);
        let queue = if play_all {
            play_all::plan(
                kind,
                replacement_mode,
                resources,
                source,
                anchor,
                &mut rand::rng(),
            )?
        } else {
            let anchor = if row_next {
                BatchAnchor::RowNext {
                    index,
                    require_admitted: kind == HistoryKind::Song,
                }
            } else if full_list && was_empty {
                BatchAnchor::ListHead
            } else if let Some(anchor) = anchor {
                BatchAnchor::VoiceBookmark(anchor)
            } else {
                BatchAnchor::Automatic
            };
            plan(queue, resources, source, start, anchor, &mut rand::rng())?
        };
        let update = HistoryQueueUpdate {
            expected: before.fence(),
            queue,
            start,
            play_all,
            row_next,
            fm_backup_queue,
            was_empty,
            kind,
            revision,
            shuffle_anchor,
        };
        // The metadata reads did not hold this lock. Prevent a room join or
        // recovered persisted room from racing the ordinary queue commit.
        let _room_gate = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        self.validate_queue_clear_room(token, None)?;
        room.check()?;
        let state = if start {
            // The prepared selection claims audio only after metadata and
            // admission. Recheck the receipt again in the queue actor.
            let history = crate::history::HistoryService::new(self.session.clone())?;
            let captured = token.clone();
            let revision = update.revision.clone();
            let setting = update
                .shuffle_anchor
                .as_ref()
                .map(|binding| binding.setting.clone());
            let authority = Cancellation::new(move || {
                !room.is_cancelled()
                    && setting
                        .as_ref()
                        .is_none_or(|setting| !setting.is_cancelled())
                    && history
                        .snapshot_for(&captured, kind)
                        .is_ok_and(|snapshot| snapshot.revision == revision)
            });
            self.submit_selection_with_source(
                token,
                PlaybackCommand::ApplyHistoryBatch(Box::new(update)),
                prepared.observed,
                None,
                Some(prepared),
                Some(authority),
            )
            .await?
        } else {
            self.submit_prepared_history_queue_only(token, prepared, update, room)
                .await?
        };
        self.session.check_generation(token)?;
        project(&state, token)
    }
}

fn history_key(row: &Value) -> Option<(PlayerResourceKind, &str)> {
    identity::collection_key(row, identity::kind(row) == PlayerResourceKind::Downloaded)
}

fn admitted(row: &Value) -> bool {
    history_key(row).is_some_and(|(kind, _)| {
        row["missing"] != true
            && (kind != PlayerResourceKind::Catalog || mapping::online_play_all_eligible(row))
    })
}

fn retain_admitted(resources: &mut Vec<Value>) {
    resources.retain(admitted);
    let mut seen = HashSet::new();
    resources.retain(|row| {
        history_key(row).is_some_and(|(kind, id)| seen.insert((kind, id.to_owned())))
    });
}

#[derive(Clone, Copy)]
enum BatchAnchor<'a> {
    Automatic,
    ListHead,
    VoiceBookmark(&'a ShuffleAnchor),
    RowNext {
        index: usize,
        require_admitted: bool,
    },
}

fn plan(
    mut queue: QueueSnapshot,
    mut resources: Vec<Value>,
    source: Value,
    start: bool,
    anchor: BatchAnchor<'_>,
    rng: &mut impl Rng,
) -> Result<QueueSnapshot> {
    use super::queue_playlist::{QUEUE_LIMIT, insert_collection_by_key};
    let list_head = resources
        .first()
        .and_then(history_key)
        .map(|(kind, id)| (kind, id.to_owned()));
    let explicit = if let BatchAnchor::RowNext {
        index,
        require_admitted,
    } = anchor
    {
        // M3085 prechecks the clicked song (not the first selected row).
        // The native projection has already applied any eligible same
        // recording, so use the admitted identity from this exact slot.
        let row = resources
            .get(index)
            .filter(|row| admitted(row))
            .and_then(history_key)
            .map(|(kind, id)| (kind, id.to_owned()));
        if row.is_none() && (require_admitted || start) {
            return Err(Error::State("clicked history resource cannot be played"));
        }
        row
    } else {
        None
    };
    retain_admitted(&mut resources);
    let incumbent = queue.selected();
    let incumbent_key = incumbent
        .as_ref()
        .and_then(|row| history_key(&row.resource));
    let keeps_incumbent = incumbent.is_some()
        && !resources
            .iter()
            .take(QUEUE_LIMIT)
            .any(|row| history_key(row) == incumbent_key && incumbent_key.is_some());
    // Keep a decoder whose source is outside a full selected batch. The
    // generic splice uses the same prefix/suffix cap for all ordinary entries.
    resources.truncate(QUEUE_LIMIT - usize::from(keeps_incumbent));
    let selected = resources
        .iter()
        .filter_map(history_key)
        .map(|(kind, id)| (kind, id.to_owned()))
        .collect::<Vec<_>>();
    let first = selected
        .first()
        .ok_or(Error::State("history selection has no playable resources"))?;
    if matches!(anchor, BatchAnchor::ListHead) && list_head.as_ref() != Some(first) {
        return Err(Error::State("the first history resource is unavailable"));
    }
    if !keeps_incumbent && let (Some(current), Some(key)) = (&incumbent, incumbent_key) {
        // A queue can contain preexisting duplicate occurrences. If the
        // current identity is in the batch, the one we move must be the
        // loaded entry, even when a later copy had the same resource key.
        queue.items.retain(|row| {
            row.entry_id == current.entry_id || history_key(&row.resource) != Some(key)
        });
        let retained: HashSet<_> = queue.items.iter().map(|row| &row.entry_id).collect();
        queue.order.retain(|id| retained.contains(id));
    }
    let mut next = insert_collection_by_key(queue, resources, source, true, true, history_key);
    if !start {
        next.current = incumbent.as_ref().map(|row| row.entry_id.clone());
        if let Some(incumbent) = incumbent {
            let stored = next
                .items
                .iter_mut()
                .find(|row| row.entry_id == incumbent.entry_id)
                .ok_or(Error::QueueChanged)?;
            *stored = incumbent;
        }
    }
    let entries: HashMap<_, _> = next
        .items
        .iter()
        .filter_map(|row| history_key(&row.resource).map(|key| (key, row.entry_id.as_str())))
        .collect();
    let entry_id =
        |key: &(PlayerResourceKind, String)| entries.get(&(key.0, key.1.as_str())).copied();
    let first = entry_id(first).ok_or(Error::QueueChanged)?;
    let explicit_start = || {
        explicit
            .as_ref()
            .and_then(entry_id)
            .map(str::to_owned)
            .ok_or(Error::State(
                "clicked history resource is outside the queue limit",
            ))
    };
    let start_id = if next.mode == PlaybackMode::Random {
        let in_batch = selected.iter().filter_map(entry_id).collect();
        let start_id = start.then(|| match anchor {
            BatchAnchor::VoiceBookmark(anchor) => next
                .items
                .iter()
                .find(|row| anchor.matches(&row.resource))
                .map(|row| row.entry_id.clone())
                .ok_or(Error::QueueChanged),
            BatchAnchor::ListHead => Ok(first.to_owned()),
            BatchAnchor::RowNext { .. } => explicit_start(),
            BatchAnchor::Automatic => selected
                .choose(rng)
                .and_then(entry_id)
                .map(str::to_owned)
                .ok_or(Error::QueueChanged),
        });
        next.order = super::local_batch::random_order(&next, &in_batch, first, !start, rng);
        start_id.transpose()?
    } else {
        start
            .then(|| {
                if matches!(anchor, BatchAnchor::RowNext { .. }) {
                    explicit_start()
                } else {
                    Ok(first.to_owned())
                }
            })
            .transpose()?
    };
    if let Some(id) = start_id {
        next.current = Some(id);
    }
    Ok(next)
}
