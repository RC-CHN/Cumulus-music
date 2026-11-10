//! Local display checkpoint; only a held, suspended visit can read it on POP.
//! Neither raw episode responses nor active route/playback receipts are stored.
use super::{Episode, EpisodeView, PodcastLikedEpisode, RETAIN_FOR, Visit};
use crate::{
    Error, Result,
    podcast::cache_generation_in,
    protocol::artwork,
    storage::{LIKED_PREFIX_KEY, LIKED_PREFIX_KIND, StateStore, StateTransaction},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    time::{SystemTime, UNIX_EPOCH},
};

const CLEAR_AFTER_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const MAX_CHECKPOINT_BYTES: usize = 8 * 1024 * 1024;

fn now_ms() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|time| u64::try_from(time.as_millis()).ok())
        .ok_or(Error::State("invalid liked page clock"))
}

fn digest(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.bytes().any(|byte| byte != b'0')
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SavedRow {
    id: String,
    song_id: Option<String>,
    name: String,
    creator: String,
    radio_id: Option<String>,
    radio_name: String,
    cover: Option<String>,
    duration_ms: u64,
    fee_type: i64,
    listener_count: Option<u64>,
    gray: bool,
    withdrawn: bool,
}

impl SavedRow {
    fn capture(episode: &Episode) -> Self {
        let item = super::project_list_row(episode);
        Self {
            id: item.voice.id,
            song_id: episode.song_id.clone(),
            name: item.voice.name,
            creator: item.voice.creator,
            radio_id: item.voice.radio_id,
            radio_name: item.voice.radio_name,
            cover: artwork::original(&json!(episode.cover_url)),
            duration_ms: episode.duration_ms,
            fee_type: item.voice.fee_type,
            listener_count: item.listener_count,
            gray: episode.raw["grayDisplay"] == true,
            withdrawn: episode.display_status_withdrawn(),
        }
    }

    fn valid(&self) -> bool {
        valid_id(&self.id)
            && self.song_id.as_deref().is_none_or(valid_id)
            && self.radio_id.as_deref().is_none_or(valid_id)
            && self.name.len() <= 512
            && self.creator.len() <= 256
            && self.radio_name.len() <= 512
            && self.cover.as_ref().is_none_or(|cover| cover.len() <= 2048)
    }

    fn matches_native_row(&self, row: &Episode) -> bool {
        self.valid()
            && self.id == row.id
            && self.song_id == row.song_id
            && self.radio_id.as_ref() == row.radio.as_ref().map(|radio| &radio.id)
            && self.duration_ms == row.duration_ms
            && self.gray == (row.raw["grayDisplay"] == true)
            && self.withdrawn == row.display_status_withdrawn()
            && self.fee_type == row.raw["programFeeType"].as_i64().unwrap_or(0)
    }

    fn presentation(self) -> PodcastLikedEpisode {
        PodcastLikedEpisode {
            voice: EpisodeView {
                id: self.id,
                name: self.name,
                creator: self.creator,
                radio_id: self.radio_id,
                radio_name: self.radio_name,
                cover_url: self.cover.and_then(|cover| artwork::square(&json!(cover))),
                duration_ms: self.duration_ms,
                fee_type: self.fee_type,
                liked: true,
                unavailable: self.gray || self.withdrawn,
                withdrawn: self.withdrawn.then_some(true),
                browse_playback_receipt: None,
            },
            listener_count: self.listener_count,
        }
    }
}

#[derive(Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
enum SavedCursor {
    Number(u64),
    Text(String),
}

impl SavedCursor {
    fn number(&self) -> Option<u64> {
        match self {
            Self::Number(number) => Some(*number),
            Self::Text(text) if valid_id(text) => text.parse().ok(),
            Self::Text(_) => None,
        }
        .filter(|number| *number > 0)
    }

    fn capture(value: &Value) -> Option<Self> {
        let cursor = match value {
            Value::Number(value) => Self::Number(value.as_u64()?),
            Value::String(value) => Self::Text(value.clone()),
            _ => return None,
        };
        cursor.number().map(|_| cursor)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Checkpoint {
    saved_at_ms: u64,
    generation: u64,
    digest: String,
    rows: Vec<SavedRow>,
    count: Option<u64>,
    cursor: Option<SavedCursor>,
    seen: HashSet<u64>,
    pages: usize,
    capped: bool,
}

impl Checkpoint {
    fn presentation(self, held: &Visit) -> Option<Vec<PodcastLikedEpisode>> {
        if self.pages != held.pages
            || self.rows.len() != held.rows.len()
            || self.seen != held.cursors
            || self.capped != held.capped
            || self.count != held.count
            || self.cursor
                != held
                    .cursor
                    .as_ref()
                    .and_then(|cursor| SavedCursor::capture(&cursor.wire))
            || !self
                .rows
                .iter()
                .zip(&held.rows)
                .all(|(saved, row)| saved.matches_native_row(row))
        {
            return None;
        }
        Some(self.rows.into_iter().map(SavedRow::presentation).collect())
    }
}

fn read(tx: &StateTransaction<'_>, owner: &str) -> Result<Option<Checkpoint>> {
    Ok(tx
        .liked_prefix_raw(owner, MAX_CHECKPOINT_BYTES as i64)?
        .and_then(|raw| serde_json::from_str(&raw).ok()))
}

pub(super) fn save(store: &StateStore, owner: &str, generation: u64, visit: &Visit) -> Result<()> {
    if visit.pages == 0 {
        return Ok(());
    }
    let cursor = if let Some(cursor) = &visit.cursor {
        let Some(saved) = SavedCursor::capture(&cursor.wire) else {
            return Ok(());
        };
        Some(saved)
    } else {
        None
    };
    let mut rows = Vec::new();
    let mut bytes = 0;
    for row in &visit.rows {
        let saved = SavedRow::capture(row);
        if !saved.valid() {
            return Ok(());
        }
        bytes += serde_json::to_vec(&saved)?.len();
        if bytes > MAX_CHECKPOINT_BYTES {
            return Ok(());
        }
        rows.push(saved);
    }
    let checkpoint = Checkpoint {
        saved_at_ms: now_ms()?,
        generation,
        digest: digest(&visit.revision),
        rows,
        count: visit.count,
        cursor,
        seen: visit.cursors.clone(),
        pages: visit.pages,
        capped: visit.capped,
    };
    let bytes = serde_json::to_vec(&checkpoint)?;
    if bytes.len() > MAX_CHECKPOINT_BYTES {
        return Ok(());
    }
    let value = serde_json::from_slice(&bytes)?;
    store.transaction(|tx| {
        if cache_generation_in(tx, owner)? != generation {
            return Err(Error::QueueChanged);
        }
        tx.put(owner, LIKED_PREFIX_KIND, LIKED_PREFIX_KEY, &value)
    })
}

pub(super) fn take(
    store: &StateStore,
    owner: &str,
    generation: u64,
    held: &Visit,
) -> Result<Option<Vec<PodcastLikedEpisode>>> {
    let expected = digest(&held.revision);
    let now = now_ms()?;
    store.transaction(|tx| {
        if cache_generation_in(tx, owner)? != generation {
            return Err(Error::QueueChanged);
        }
        let Some(checkpoint) = read(tx, owner)? else {
            return Ok(None);
        };
        let Some(age) = now.checked_sub(checkpoint.saved_at_ms) else {
            return Ok(None);
        };
        if age >= RETAIN_FOR.as_millis() as u64
            || checkpoint.generation != generation
            || checkpoint.digest != expected
        {
            return Ok(None);
        }
        let Some(presentation) = checkpoint.presentation(held) else {
            return Ok(None);
        };
        tx.delete(owner, LIKED_PREFIX_KIND, LIKED_PREFIX_KEY)?;
        Ok(Some(presentation))
    })
}

pub(super) fn prune(store: &StateStore) -> Result<usize> {
    let Some(cutoff) = now_ms()?.checked_sub(CLEAR_AFTER_MS) else {
        return Ok(0);
    };
    store.prune_liked_prefixes(cutoff)
}
