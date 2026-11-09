//! LocalMusicBatch Play and Add (M3085 -> M308 -> M1384).
use super::{
    MusicClient, PlaybackStatus, PlayerResourceKind,
    identity::collection_key,
    queue_playlist::{QUEUE_LIMIT, insert_collection_bound},
};
use crate::{
    Error, Result,
    account::SessionToken,
    client::local::{LocalTrackReference, track_reference},
    library::catalog::{LocalGroup, LocalQuery},
    playback::{PlaybackMode, QueueSnapshot, controller::LocalBatchUpdate, engine::Cancellation},
};
use rand::{Rng, seq::IndexedRandom};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LocalBatchAction {
    Play,
    Add,
}

impl MusicClient {
    pub(super) async fn local_batch_selection(
        &self,
        token: &SessionToken,
        mut query: LocalQuery,
        revision: String,
        tracks: Vec<LocalTrackReference>,
        action: LocalBatchAction,
    ) -> Result<(LocalBatchUpdate, Cancellation)> {
        query.validate()?;
        if query.group != LocalGroup::List
            || query.offset != 0
            || query.revision.is_some()
            || revision.len() != 64
            || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !(1..=50_000).contains(&tracks.len())
        {
            return Err(Error::InvalidInput("invalid local batch reference"));
        }
        for track in &tracks {
            track_reference(&track.id, &track.revision)?;
        }
        let room = self.together_detached_guard(token);
        room.check()?;
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(token).await?;
        if before.fm || queue.intelligence.is_some() {
            return Err(Error::State("local batch requires an ordinary queue"));
        }
        if matches!(
            before.state,
            PlaybackStatus::Preparing | PlaybackStatus::Loading
        ) {
            return Err(Error::MutationBusy);
        }

        query.revision = Some(revision.clone());
        let local = self.local.context().await?;
        let (resources, index) = tokio::task::spawn_blocking(move || {
            let rows: Vec<_> = tracks
                .iter()
                .map(|row| (row.id.as_str(), row.revision.as_str()))
                .collect();
            local
                .library
                .prepare_batch_playback(&query, &rows, QUEUE_LIMIT)
        })
        .await
        .map_err(|_| Error::State("local batch worker stopped"))??;
        room.check()?;
        self.session.check_generation(token)?;
        let start = action == LocalBatchAction::Play || queue.items.is_empty();
        let queue = insert_local_batch(
            queue,
            resources,
            json!({"kind":"local","revision":revision}),
            start,
            &mut rand::rng(),
        )?;
        Ok((
            LocalBatchUpdate {
                expected: before.fence(),
                queue,
                index,
                start,
            },
            room,
        ))
    }
}

fn insert_local_batch(
    mut queue: QueueSnapshot,
    resources: Vec<Value>,
    source: Value,
    start: bool,
    rng: &mut impl Rng,
) -> Result<QueueSnapshot> {
    let inserted: Vec<_> = resources
        .iter()
        .map(|row| {
            row["local_id"]
                .as_str()
                .map(str::to_owned)
                .ok_or(Error::QueueChanged)
        })
        .collect::<Result<_>>()?;
    let random = queue.mode == PlaybackMode::Random;
    let retained = (!start).then(|| queue.selected()).flatten();
    if let Some(current) = &retained
        && let Some((PlayerResourceKind::Local, id)) = collection_key(&current.resource, false)
        && let Some(requested) = resources.iter().find(|row| row["local_id"] == id)
        && requested["local_revision"] != current.resource["local_revision"]
    {
        return Err(Error::State(
            "local batch would replace the current file revision",
        ));
    }
    // Play can discard the previous queue when the official cap is filled.
    // Add reserves its current row when it is outside the incoming batch.
    if start && resources.len() == QUEUE_LIMIT {
        queue.items.clear();
        queue.order.clear();
        queue.current = None;
        queue.more = None;
    }
    let mut queue = insert_collection_bound(queue, resources, source, false, true, true);
    if !start {
        queue.current = retained.as_ref().map(|entry| entry.entry_id.clone());
        if let Some(current) = retained {
            // Reposition an exact duplicate but never rebind the current
            // decoder, source metadata or native playback entry.
            let stored = queue
                .items
                .iter_mut()
                .find(|row| row.entry_id == current.entry_id)
                .ok_or(Error::QueueChanged)?;
            *stored = current;
        }
    }
    let entries: HashMap<_, _> = queue
        .items
        .iter()
        .filter_map(|row| match collection_key(&row.resource, false) {
            Some((PlayerResourceKind::Local, id)) => Some((id, &row.entry_id)),
            _ => None,
        })
        .collect();
    let first = entries
        .get(inserted.first().ok_or(Error::QueueChanged)?.as_str())
        .ok_or(Error::QueueChanged)?;
    if start {
        let selected = if random {
            inserted.choose(rng)
        } else {
            inserted.first()
        }
        .ok_or(Error::QueueChanged)?;
        queue.current = Some((*entries.get(selected.as_str()).ok_or(Error::QueueChanged)?).clone());
    }
    if random {
        let in_batch: HashSet<_> = inserted
            .iter()
            .filter_map(|id| entries.get(id.as_str()).map(|entry| entry.as_str()))
            .collect();
        queue.order = random_order(&queue, &in_batch, first, !start, rng);
    }
    Ok(queue)
}

pub(super) fn random_order(
    queue: &QueueSnapshot,
    inserted: &HashSet<&str>,
    first_inserted: &str,
    queue_only: bool,
    rng: &mut impl Rng,
) -> Vec<String> {
    let mut last_retained = None;
    // M213.c gives each row a uniform integer ×100. M176.b then assigns
    // the first inserted entry the last retained display row's key + 1.
    let mut entries: Vec<_> = queue
        .items
        .iter()
        .map(|entry| {
            let key = rng.random_range(0_u64..=99_999_999) * 100;
            if !inserted.contains(entry.entry_id.as_str()) {
                last_retained = Some(key);
            }
            (&entry.entry_id, key)
        })
        .collect();
    let mut next_anchor = None;
    if let Some(first) = entries
        .iter()
        .position(|(id, _)| id.as_str() == first_inserted)
    {
        if let Some(last) = last_retained {
            entries[first].1 = last + 1;
        }
        // M1384 places the first new row immediately after its display
        // predecessor in random traversal when there is a current row. Rust
        // persists relative order, so use the predecessor's fresh random key
        // in place of the desktop's old scalar. When current itself was moved
        // within the batch the predecessor need not be the current row.
        if queue_only && queue.current.is_some() && first > 0 {
            let (anchor, key) = entries[first - 1];
            next_anchor = Some(anchor.as_str());
            entries[first].1 = key + 1;
        }
    }
    // The official next lookup compares keys strictly. Put equal-key rows
    // before the anchor so they cannot jump ahead of its explicit successor.
    entries.sort_by_key(|(id, key)| (*key, next_anchor == Some(id.as_str())));
    entries.into_iter().map(|(id, _)| id.clone()).collect()
}
