use super::dto::*;
use crate::{
    Result,
    client::{catalog::cards, mapping, podcast::view as podcast},
    history::{HistoryAction, HistoryKind, HistoryMutation, HistoryRecord, RecordOrigin},
    podcast::{Episode, Radio},
};

pub(super) fn entry(
    kind: HistoryKind,
    record: HistoryRecord,
    available: bool,
) -> Result<HistoryEntry> {
    let value = &record.resource;
    let resource = match kind {
        HistoryKind::Song => HistoryResource::Song {
            track: if let Some(id) = value["local_id"].as_str() {
                let metadata = serde_json::from_value(value["metadata"].clone())?;
                super::super::local::view::track(id.into(), &metadata, !available)
            } else {
                mapping::track(value)?
            },
        },
        HistoryKind::Album => HistoryResource::Album {
            album: cards::album(value)?,
        },
        HistoryKind::Playlist => HistoryResource::Playlist {
            playlist: HistoryPlaylistCard {
                id: record.id.clone(),
                name: mapping::string(&value["name"]),
                description: mapping::string(&value["description"]),
                creator: mapping::string(&value["creator"]["nickname"]),
                cover_url: mapping::artwork(&value["coverImgUrl"]),
                track_count: value["trackCount"].as_u64().unwrap_or(0),
            },
        },
        HistoryKind::Voice => HistoryResource::Voice {
            voice: podcast::episode(&Episode::from_official(value)?, &[]),
        },
        HistoryKind::Podcast => HistoryResource::Podcast {
            radio: podcast::card(&Radio::from_official(value)?),
        },
        HistoryKind::Audiobook => HistoryResource::Audiobook {
            radio: podcast::card(&Radio::from_official(value)?),
        },
    };
    let device = if record.origin == RecordOrigin::Local {
        "本机"
    } else {
        match record
            .device
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "pc" | "windows" | "win" => "电脑",
            "ios" | "iphone" | "ipad" => "iOS",
            "android" => "Android",
            "mac" | "osx" | "macos" => "Mac",
            "web" => "网页",
            _ => "其他设备",
        }
    };
    Ok(HistoryEntry {
        id: record.id,
        played_at: record.played_at,
        device,
        origin: record.origin,
        play_progress: record.play_progress,
        listened: record.listened,
        resource,
    })
}
pub(super) fn operation(op: HistoryMutation) -> HistoryOperationView {
    let (action, selected_count) = match op.action {
        HistoryAction::Clear => ("clear", None),
        HistoryAction::Delete { ids } => ("delete", Some(ids.len())),
    };
    HistoryOperationView {
        id: op.id,
        action,
        selected_count,
        phase: op.phase,
        created_at: op.created_at,
        local_removed: op.local_removed,
        remote_planned: op.remote_planned,
        last_read_at: op.last_read_at,
    }
}
