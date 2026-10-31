use super::*;
use crate::{
    listen_together::PlayState,
    playback::{
        QueueEntry, QueueSnapshot,
        controller::{PlaybackStatus, QueueTransport, QueueUpdate},
    },
};
use std::collections::BTreeMap;

pub(super) fn build(
    receipt: &SharedReception,
    before: &PlaybackSnapshot,
    old: &QueueSnapshot,
    shared: &selection::Shareable,
    cards: &BTreeMap<String, serde_json::Value>,
) -> Result<QueueUpdate> {
    let snapshot = receipt
        .snapshot
        .as_ref()
        .ok_or(Error::State("shared reception has no snapshot"))?;
    let playlist = snapshot
        .playlist
        .as_ref()
        .ok_or(Error::State("shared queue missing"))?;
    let access = receipt
        .privileges
        .iter()
        .map(|p| (p.track_id.as_str(), p))
        .collect::<BTreeMap<_, _>>();
    let mut items = vec![];
    let mut entries = BTreeMap::new();
    for id in &playlist.display {
        let privilege = access
            .get(id.as_str())
            .ok_or(Error::State("shared privilege missing"))?;
        let existing = old
            .current
            .iter()
            .chain(old.items.iter().map(|e| &e.entry_id))
            .find(|entry| shared.entries.get(*entry) == Some(id))
            .and_then(|entry| old.items.iter().find(|e| &e.entry_id == entry));
        let mut entry = if let Some(existing) = existing {
            existing.clone()
        } else {
            let song = cards
                .get(id)
                .ok_or(Error::Response("shared song detail missing"))?;
            QueueEntry {
                entry_id: uuid::Uuid::new_v4().simple().to_string(),
                resource: json!({"id":id,"song":song,"dt":song["dt"],"level":before.requested_quality.as_deref().unwrap_or("standard")}),
                source: json!({"kind":"listen_together"}),
            }
        };
        let resource = entry
            .resource
            .as_object_mut()
            .ok_or(Error::State("invalid queued resource"))?;
        match privilege.constraint() {
            Some(constraint) => {
                resource.insert(
                    "playback_constraint".into(),
                    serde_json::to_value(constraint)?,
                );
                resource.insert("shared_room".into(), json!(receipt.room_id));
            }
            None => {
                resource.remove("playback_constraint");
                resource.remove("shared_room");
            }
        }
        entries.insert(id.clone(), entry.entry_id.clone());
        items.push(entry);
    }
    let ordered = if playlist.mode == SharedMode::Random {
        playlist
            .random
            .as_ref()
            .ok_or(Error::State("shared random order missing"))?
    } else {
        &playlist.display
    };
    let order = ordered
        .iter()
        .map(|id| {
            entries
                .get(id)
                .cloned()
                .ok_or(Error::State("invalid shared order"))
        })
        .collect::<Result<Vec<_>>>()?;
    let requested = if receipt.ignore_play_command {
        shared
            .current
            .as_ref()
            .filter(|id| entries.contains_key(*id))
            .or_else(|| snapshot.play_command.as_ref().map(|c| &c.target_song_id))
    } else {
        snapshot
            .play_command
            .as_ref()
            .map(|c| &c.target_song_id)
            .or(shared.current.as_ref())
    };
    let start = requested
        .and_then(|id| ordered.iter().position(|v| v == id))
        .unwrap_or(0);
    let target = (0..ordered.len())
        .map(|n| &ordered[(start + n) % ordered.len()])
        .find(|id| access.get(id.as_str()).is_some_and(|p| p.can_play()));
    let current = target.and_then(|id| entries.get(id)).cloned();
    let transport = target.map(|target| {
        let same = shared.current.as_ref() == Some(target);
        let command = snapshot
            .play_command
            .as_ref()
            .filter(|c| &c.target_song_id == target);
        let preserve = same && receipt.ignore_play_command;
        let mut position = if preserve {
            before.position.unwrap_or(0.0)
        } else {
            command.map_or(0.0, |c| c.progress as f64 / 1000.0)
        };
        if let Some(preview) = access[target.as_str()].preview
            && (position < preview.start || position >= preview.end)
        {
            position = preview.start;
        }
        let paused = if preserve {
            !matches!(
                before.state,
                PlaybackStatus::Playing | PlaybackStatus::Buffering
            )
        } else {
            command.map_or(
                !matches!(
                    before.state,
                    PlaybackStatus::Playing | PlaybackStatus::Buffering
                ),
                |c| c.play_status == PlayState::Pause,
            )
        };
        QueueTransport {
            position: Some(position),
            paused,
        }
    });
    Ok(QueueUpdate {
        expected: before.fence(),
        queue: QueueSnapshot {
            items,
            order,
            current,
            mode: match playlist.mode {
                SharedMode::Random => PlaybackMode::Random,
                SharedMode::SingleLoop => PlaybackMode::SingleLoop,
                SharedMode::OrderLoop => PlaybackMode::OrderLoop,
            },
            fm_source: None,
        },
        transport,
    })
}
