//! Native origin, row markers and read inputs for the official more-recommendations lane.
use crate::{catalog::queue_more::QueueMoreSong, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashSet};

use super::{QueueEntry, QueueSnapshot};

pub(super) mod anchor;

pub(crate) const KIND: &str = "queue-more-v1";
pub(crate) const PREFERENCE: &str = "enabled";
pub(crate) const ROOM_BINDING: &str = "listen-together-player-v1";
const SCENE: &str = "queue_more_recommendation";
pub(crate) const PLAYLIST_MEMBERSHIP: &str = "queue_more_playlist_membership";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlaylistMembership {
    Liked,
    Created,
    Subscribed,
    Other,
    Unknown,
}

impl PlaylistMembership {
    fn eligible(self, assignment: Option<&str>) -> bool {
        if assignment == Some("t4") {
            return true;
        }
        match self {
            Self::Liked => assignment == Some("t3"),
            Self::Created | Self::Unknown => false,
            Self::Subscribed => matches!(assignment, Some("t2" | "t3")),
            Self::Other => true,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Origin {
    Daily,
    Style,
    HistoryDaily,
    Playlist(PlaylistMembership),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct QueueMoreOrigin {
    origin: Origin,
    pub(crate) nonce: [u8; 16],
    pub(crate) prefetch_on_open: bool,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) active: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "anchor::Anchor::is_legacy")]
    anchor: anchor::Anchor,
}
impl QueueMoreOrigin {
    pub(crate) fn new(source: &Value) -> Option<Self> {
        let origin = match source["kind"].as_str()? {
            "daily_recommendations" => Origin::Daily,
            "style_recommendations" => Origin::Style,
            "member" if source["query"]["kind"] == "history" => Origin::HistoryDaily,
            "playlist" if identifier(&source["id"]).is_ok() => {
                Origin::Playlist(serde_json::from_value(source[PLAYLIST_MEMBERSHIP].clone()).ok()?)
            }
            _ => return None,
        };
        Some(Self {
            origin,
            nonce: *uuid::Uuid::new_v4().as_bytes(),
            prefetch_on_open: true,
            active: BTreeSet::new(),
            anchor: anchor::Anchor::None,
        })
    }

    pub(crate) fn eligible(&self, playlist_assignment: Option<&str>) -> bool {
        match self.origin {
            Origin::Playlist(membership) => membership.eligible(playlist_assignment),
            _ => true,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueMoreView {
    pub visible: bool,
    pub enabled: bool,
    pub can_toggle: bool,
    pub separator_before: Option<String>,
    pub prefetch_on_open: bool,
}

pub(crate) fn active(queue: &QueueSnapshot, entry: &QueueEntry) -> bool {
    entry.source["kind"] == SCENE
        && queue
            .more
            .as_ref()
            .is_some_and(|state| state.active.contains(&entry.entry_id))
}

pub(crate) fn separator(queue: &QueueSnapshot) -> Option<String> {
    anchor::index_and_id(queue).map(|(_, id)| id.to_owned())
}

pub(crate) fn clear_active(queue: &mut QueueSnapshot) {
    let Some(state) = &mut queue.more else { return };
    state.anchor = anchor::Anchor::None;
    let active = std::mem::take(&mut state.active);
    let current = queue.current.as_ref();
    let mut removed = HashSet::new();
    queue.items.retain(|entry| {
        if !active.contains(&entry.entry_id) {
            return true;
        }
        if Some(&entry.entry_id) == current {
            true
        } else {
            removed.insert(entry.entry_id.clone());
            false
        }
    });
    queue.order.retain(|id| !removed.contains(id));
}

pub(crate) fn normalize(queue: &mut QueueSnapshot) {
    if queue.items.is_empty() || queue.fm_source.is_some() || queue.intelligence.is_some() {
        queue.more = None;
    } else if let Some(state) = &mut queue.more {
        let ids: HashSet<_> = queue
            .items
            .iter()
            .filter(|entry| entry.source["kind"] == SCENE)
            .map(|entry| entry.entry_id.as_str())
            .collect();
        state.active.retain(|id| ids.contains(id.as_str()));
        anchor::normalize(queue);
    }
}

/// Normalize only an already attached song's catalog ID. A download task,
/// voice ID or local file identity cannot become a recommendation seed.
pub(crate) fn seed(entry: &QueueEntry) -> Option<String> {
    if !entry.visible_in_queue() || entry.resource.get("voice_id").is_some() {
        return None;
    }
    identifier(&entry.resource["song"]["id"]).ok()
}

pub(crate) fn recommended(song: QueueMoreSong, level: &str) -> QueueEntry {
    let privileges = song.privilege.map(|right| {
        json!({"id":song.id,"st":right.status,"fee":right.fee,
            "flag":right.paid_flags,"pl":right.play_bitrate})
    });
    let mut track = json!({"id":song.id,"name":song.name,"dt":song.duration_ms,
        "fee":song.fee,"ar":song.artists.iter().map(|artist| json!({
            "id":artist.id,"name":artist.name})).collect::<Vec<_>>(),
        "al":{"id":song.album_id,"name":song.album_name,"picUrl":song.cover_url}});
    song.title_names.write_queue_fields(&mut track);
    if let Some(privileges) = privileges {
        track["privilege"] = privileges;
    }
    QueueEntry {
        entry_id: uuid::Uuid::new_v4().to_string(),
        resource: json!({"id":song.id,"level":level,"dt":song.duration_ms,"song":track}),
        source: json!({"kind":SCENE}),
    }
}
