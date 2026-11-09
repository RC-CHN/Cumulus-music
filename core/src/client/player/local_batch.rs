//! LocalMusicBatch's explicit Play (M3085 -> M308 -> M1384).
use super::{
    MusicClient, PlaybackCommand, PlaybackStatus, PlayerResourceKind,
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

impl MusicClient {
    pub(super) async fn local_batch_selection(
        &self,
        token: &SessionToken,
        mut query: LocalQuery,
        revision: String,
        tracks: Vec<LocalTrackReference>,
    ) -> Result<(PlaybackCommand, Cancellation)> {
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
        let queue = insert_local_batch(
            queue,
            resources,
            json!({"kind":"local","revision":revision}),
            &mut rand::rng(),
        )?;
        Ok((
            PlaybackCommand::PlayLocalBatch(LocalBatchUpdate {
                expected: before.fence(),
                queue,
                index,
            }),
            room,
        ))
    }
}

fn insert_local_batch(
    mut queue: QueueSnapshot,
    resources: Vec<Value>,
    source: Value,
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
    let selected = if random {
        inserted.choose(rng)
    } else {
        inserted.first()
    }
    .ok_or(Error::QueueChanged)?;
    // The official limit applies to the new group before retaining the old
    // list. At exactly 10,000, even an unrelated prior current is evicted.
    if resources.len() == QUEUE_LIMIT {
        queue.items.clear();
        queue.order.clear();
        queue.current = None;
        queue.more = None;
    }
    let mut queue = insert_collection_bound(queue, resources, source, false, true, true);
    let entries: HashMap<_, _> = queue
        .items
        .iter()
        .filter_map(|row| match collection_key(&row.resource, false) {
            Some((PlayerResourceKind::Local, id)) => Some((id, &row.entry_id)),
            _ => None,
        })
        .collect();
    let selected = entries.get(selected.as_str()).ok_or(Error::QueueChanged)?;
    queue.current = Some((*selected).clone());
    if random {
        let first = entries
            .get(inserted[0].as_str())
            .ok_or(Error::QueueChanged)?;
        let in_batch: HashSet<_> = inserted
            .iter()
            .map(|id| {
                entries
                    .get(id.as_str())
                    .map(|entry| entry.as_str())
                    .ok_or(Error::QueueChanged)
            })
            .collect::<Result<_>>()?;
        queue.order = random_order(&queue, &in_batch, first, rng);
    }
    Ok(queue)
}

fn random_order(
    queue: &QueueSnapshot,
    inserted: &HashSet<&str>,
    first_inserted: &str,
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
    if let Some(last) = last_retained
        && let Some((_, key)) = entries
            .iter_mut()
            .find(|(id, _)| id.as_str() == first_inserted)
    {
        *key = last + 1;
    }
    entries.sort_by_key(|(_, key)| *key);
    entries.into_iter().map(|(id, _)| id.clone()).collect()
}
