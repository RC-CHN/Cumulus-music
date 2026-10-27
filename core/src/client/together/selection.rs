use super::*;
use crate::{
    library::LocalLibrary,
    playback::{PlaybackMode, QueueSnapshot},
    protocol::api::identifier,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Shareable {
    pub cards: BTreeMap<String, serde_json::Value>,
    pub entries: BTreeMap<String, String>,
    pub display: Vec<String>,
    pub random: Vec<String>,
    pub current: Option<String>,
    pub mode: SharedMode,
}
pub(super) fn shared_selection(
    session: &AccountSession,
    token: &SessionToken,
    store: Arc<StateStore>,
    queue: &QueueSnapshot,
) -> Result<Shareable> {
    let library = LocalLibrary::new(store)?;
    let local_ids = queue
        .items
        .iter()
        .filter_map(|i| i.resource["local_id"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let mut links = BTreeMap::new();
    for ids in local_ids.chunks(500) {
        links.extend(library.online_mappings(session, token, ids)?);
    }
    let mut entries = BTreeMap::new();
    let mut cards = BTreeMap::new();
    for item in &queue.items {
        let r = &item.resource;
        if !r["voice_id"].is_null() || !r["song"]["voice_id"].is_null() {
            continue;
        }
        let id = if let Some(local) = r["local_id"].as_str() {
            links.get(local).map(|m| m.song_id.clone())
        } else {
            identifier(&r["id"]).ok()
        };
        if let Some(id) = id {
            let song = if let Some(local) = r["local_id"].as_str() {
                links.get(local).and_then(|m| m.song.as_ref())
            } else {
                r.get("song")
            };
            if let Some(song) = song.filter(|s| s.is_object()) {
                cards.insert(id.clone(), song.clone());
            }
            entries.insert(item.entry_id.clone(), id);
        }
    }
    let current = queue
        .current
        .as_ref()
        .and_then(|id| entries.get(id))
        .cloned();
    let all = queue
        .items
        .iter()
        .filter_map(|i| entries.get(&i.entry_id).cloned())
        .collect::<Vec<_>>();
    let display = crate::listen_together::shared_window(&all, current.as_deref())?;
    let mut seen = BTreeSet::new();
    let random = queue
        .order
        .iter()
        .filter_map(|id| entries.get(id))
        .filter(|id| display.contains(id) && seen.insert((*id).clone()))
        .cloned()
        .collect();
    Ok(Shareable {
        cards,
        entries,
        display,
        random,
        current,
        mode: match queue.mode {
            PlaybackMode::Random => SharedMode::Random,
            PlaybackMode::SingleLoop => SharedMode::SingleLoop,
            _ => SharedMode::OrderLoop,
        },
    })
}
