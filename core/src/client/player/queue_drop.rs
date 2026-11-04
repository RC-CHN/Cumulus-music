//! Local library row dropped onto a displayed ordinary queue boundary.
use super::*;
use crate::{
    library::catalog::LocalQuery,
    playback::{PlaybackMode, QueueEntry, QueueSnapshot, controller::QueueUpdate},
};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

pub(super) struct LocalDrop {
    pub query: LocalQuery,
    pub revision: String,
    pub track_id: String,
    pub expected_queue_revision: String,
    pub expected_play_id: Option<String>,
    pub before_entry_id: Option<String>,
}

impl MusicClient {
    pub(super) async fn drop_local_into_queue(
        &self,
        epoch: &str,
        mut drop: LocalDrop,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        self.session.check_generation(&token)?;
        let controller = self.player.controller().await?;
        let (before, mut queue) = controller.queue_with_state(&token).await?;
        if before.queue_revision.to_string() != drop.expected_queue_revision
            || before.play_id != drop.expected_play_id
        {
            return Err(Error::QueueChanged);
        }
        if before.startup_pending
            || before.fm
            || queue.intelligence.is_some()
            || queue.mode == PlaybackMode::Random
            || queue.items.is_empty()
            || queue.items.iter().any(|item| !item.visible_in_queue())
            || matches!(
                before.state,
                PlaybackStatus::Preparing | PlaybackStatus::Loading
            )
            || matches!(
                before.state,
                PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
            ) && before.timeline.is_none()
            || self.together_attached(&token, None)?
        {
            return Err(Error::State(
                "local drops require a stable ordinary ordered queue",
            ));
        }
        let boundary = match drop.before_entry_id.as_deref() {
            Some(id) => queue
                .items
                .iter()
                .position(|entry| entry.entry_id == id)
                .ok_or(Error::InvalidInput(
                    "drop boundary is outside displayed queue",
                ))?,
            None => queue.items.len(),
        };
        let attempt = self.player.selection.load(Ordering::Acquire);
        drop.query.revision = Some(drop.revision.clone());
        let local = self.local.context().await?;
        let resource = tokio::task::spawn_blocking(move || {
            let snapshot = local.library.catalog(&drop.query)?;
            if !snapshot
                .items
                .iter()
                .skip(drop.query.offset)
                .take(drop.query.limit)
                .any(|row| row.id == drop.track_id)
            {
                return Err(Error::InvalidInput(
                    "local drop row is outside the displayed page",
                ));
            }
            snapshot.queue_track(&drop.track_id)
        })
        .await
        .map_err(|_| Error::State("local drop worker stopped"))??;
        self.session.check_generation(&token)?;
        let selection = self.player.selection.clone();
        if selection.load(Ordering::Acquire) != attempt {
            return Err(Error::StaleResponse);
        }
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        if self.together_attached(&token, None)?
            || self.queue_clear_context(&token)?.room_id.is_some()
        {
            return Err(Error::State("local drops require ordinary playback"));
        }
        let source = json!({"kind":"local","revision":drop.revision});
        insert_local_drop(&mut queue, resource, source, boundary)?;
        let expected = before.fence();
        let value = controller
            .command_guarded(
                &token,
                PlaybackCommand::UpdateQueue(QueueUpdate {
                    expected: expected.clone(),
                    queue,
                    transport: None,
                }),
                expected,
                Cancellation::new(move || selection.load(Ordering::Acquire) == attempt),
            )
            .await?;
        self.session.check_generation(&token)?;
        project(&value, &token)
    }
}

fn insert_local_drop(
    queue: &mut QueueSnapshot,
    resource: Value,
    source: Value,
    mut boundary: usize,
) -> Result<()> {
    let local_id = resource["local_id"]
        .as_str()
        .ok_or(Error::State("drop resource is not local"))?;
    let mut duplicates = queue
        .items
        .iter()
        .filter(|entry| entry.resource["local_id"].as_str() == Some(local_id));
    let first = duplicates.next();
    let current = first
        .filter(|entry| queue.current.as_ref() == Some(&entry.entry_id))
        .or_else(|| duplicates.find(|entry| queue.current.as_ref() == Some(&entry.entry_id)));
    let inserted = if let Some(current) = current {
        // Moving the active duplicate keeps its exact loaded bytes and play ID.
        current.clone()
    } else if let Some(first) = first {
        QueueEntry {
            entry_id: first.entry_id.clone(),
            resource: resource.clone(),
            source,
        }
    } else {
        if queue.items.len() >= super::queue_playlist::QUEUE_LIMIT {
            return Err(Error::State("drop would exceed queue capacity"));
        }
        QueueEntry {
            entry_id: uuid::Uuid::new_v4().to_string(),
            resource: resource.clone(),
            source,
        }
    };
    let mut index = 0;
    let original_boundary = boundary;
    queue.items.retain(|entry| {
        let duplicate = entry.resource["local_id"].as_str() == Some(local_id);
        if duplicate && index < original_boundary {
            boundary -= 1;
        }
        index += 1;
        !duplicate
    });
    queue.items.insert(boundary, inserted);
    queue.order = queue
        .items
        .iter()
        .map(|entry| entry.entry_id.clone())
        .collect();
    Ok(())
}
