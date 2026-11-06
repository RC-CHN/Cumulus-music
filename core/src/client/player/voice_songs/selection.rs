//! Receipt-scoped multi-row actions for the episode song table.
use super::{MAX_TRACKS, actions::Page};
use crate::{
    Error, Result,
    client::player::{
        PlayerResourceKind, identity,
        queue_playlist::{QUEUE_LIMIT, insert_collection_bound},
    },
    membership::Quality,
    playback::{
        QueueSnapshot,
        controller::{PlaybackCommand, PlaybackFence, QueueTransport, QueueUpdate},
    },
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSongSelectionKind {
    MenuPlay,
    NextPlay,
    DoubleClick,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceSongSelectionAction {
    pub kind: VoiceSongSelectionKind,
    pub index: usize,
    /// Distinct card ordinals in the selection order (M106:291), including
    /// the interaction's anchor. Repeated song IDs are legal card rows.
    pub indices: Vec<usize>,
}

impl VoiceSongSelectionAction {
    pub(super) fn command(
        self,
        page: &Page,
        quality: Quality,
        replace: bool,
        mut queue: QueueSnapshot,
        fence: &PlaybackFence,
        source: Value,
    ) -> Result<PlaybackCommand> {
        const INVALID: Error = Error::InvalidInput("invalid vinyl episode song selection");
        if !(2..=MAX_TRACKS).contains(&self.indices.len()) {
            return Err(INVALID);
        }
        let clicked = page.rows.get(self.index).ok_or(INVALID)?;
        let mut seen_indices = [false; MAX_TRACKS];
        let mut rows = self
            .indices
            .iter()
            .map(|&index| {
                let row = page.rows.get(index).ok_or(INVALID)?;
                if std::mem::replace(seen_indices.get_mut(index).ok_or(INVALID)?, true) {
                    return Err(INVALID);
                }
                Ok(row)
            })
            .collect::<Result<Vec<_>>>()?;
        if !seen_indices.get(self.index).copied().unwrap_or(false) {
            return Err(INVALID);
        }

        if self.kind == VoiceSongSelectionKind::DoubleClick && !replace {
            // M3085 ignores selectedData when the double-click preference is
            // add. Use the same one-row eligibility as the single-row action.
            rows = vec![clicked];
        } else {
            // Official menu availability is group-wide, but M3085 still
            // checks the trigger. Here only ordinary verified online songs
            // can be used; neither alternate nor downloaded playback is inferred.
            if clicked.copyright_blocked {
                return Err(Error::State("vinyl episode song is unavailable online"));
            }
            let mut seen_songs = HashSet::new();
            rows.retain(|row| {
                row.online_eligible
                    && !row.copyright_blocked
                    && seen_songs.insert(row.track.id.as_str())
            });
        }

        let resources = rows.iter().map(|row| row.resource(quality)).collect();
        if replace {
            let index = rows
                .iter()
                .position(|row| row.track.id == clicked.track.id)
                .ok_or(Error::State("vinyl episode song is unavailable online"))?;
            return Ok(PlaybackCommand::Replace {
                resources,
                index,
                source,
            });
        }

        let catalog_ids: HashSet<_> = queue
            .items
            .iter()
            .filter_map(
                |entry| match identity::collection_key(&entry.resource, false) {
                    Some((PlayerResourceKind::Catalog, id)) => Some(id),
                    _ => None,
                },
            )
            .collect();
        let additional = rows
            .iter()
            .filter(|row| !catalog_ids.contains(row.track.id.as_str()))
            .count();
        if queue.items.len().saturating_add(additional) > QUEUE_LIMIT {
            return Err(Error::State("playback queue is full"));
        }

        let is_next = self.kind == VoiceSongSelectionKind::NextPlay;
        let previous = is_next.then(|| queue.clone());
        queue = insert_collection_bound(queue, resources, source, false, true, !is_next);
        if !is_next {
            queue.current = Some(
                queue
                    .items
                    .iter()
                    .find(|entry| identity::online_song(&entry.resource, &clicked.track.id))
                    .ok_or(Error::State("vinyl episode song was not inserted"))?
                    .entry_id
                    .clone(),
            );
        }
        if previous.as_ref() == Some(&queue) {
            return Ok(PlaybackCommand::Snapshot);
        }
        Ok(PlaybackCommand::UpdateQueue(QueueUpdate {
            expected: fence.clone(),
            queue,
            transport: (!is_next).then_some(QueueTransport {
                position: None,
                paused: false,
            }),
        }))
    }
}
