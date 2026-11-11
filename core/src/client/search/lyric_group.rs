//! Native authority for one dedicated Lyrics result and its contiguous pages.
use super::{SearchCategory, SearchItem, SearchNextRight, SearchResultQuery, SearchResultView};
use crate::{
    Error, Result,
    account::SessionToken,
    client::dto::TrackView,
    protocol::{
        api::{Api, identifier},
        wire::serialization::js_truthy,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use uuid::Uuid;

const MAX_GROUP: usize = 500;
const MAX_OBSERVED: usize = 10_100; // last valid offset plus the bounded remote page
const LIFETIME: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricGroupOperation {
    Play,
    Next,
}

#[derive(Serialize)]
pub struct LyricGroupPlaybackRow {
    receipt: String,
    right: SearchNextRight,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Rights {
    kind: SearchNextRight,
    status: f64,
    bitrate: f64,
    flags: i64,
    duration_ms: u64,
}

pub(super) struct Observation {
    id: String,
    right: Option<Rights>,
}

pub(super) fn observe(raw: &Value, track: Option<&TrackView>) -> Option<Observation> {
    let song = raw.get("song")?;
    let id = identifier(&song["id"]).ok()?;
    if id.len() > 128 || id.starts_with('0') {
        return None;
    }
    let right = track.and_then(|track| {
        if super::song_comment::observe_lyric(raw, track).as_deref() != Some(id.as_str())
            || !super::queue_drop::bounded_track(track)
            || [song, raw].iter().any(|source| {
                [
                    "isOfflineDownOnline",
                    "audio",
                    "radio",
                    "noCopyrightRcmd",
                    "noCopyrightRcmds",
                ]
                .iter()
                .any(|key| js_truthy(&source[*key]))
            })
        {
            return None;
        }
        let normalized = |privilege: &Value| {
            let kind =
                SearchNextRight::from_song(&serde_json::json!({"privilege": privilege}), &id)?;
            Some(Rights {
                kind,
                status: super::privilege::status(&privilege["st"])?,
                bitrate: super::privilege::status(&privilege["pl"])?,
                flags: match &privilege["flag"] {
                    Value::Null => 0,
                    value => value
                        .as_i64()
                        .or_else(|| value.as_str()?.trim().parse().ok())?,
                },
                duration_ms: track.duration_ms,
            })
        };
        // M15.Wn spreads the outer hit then its nested song. Use the outer
        // privilege only if the nested field is absent. Contradictory proofs
        // are not authority, even when both expose the same numeric song ID.
        let outer = raw.get("privilege");
        let nested = song.get("privilege");
        let right = normalized(nested.or(outer)?)?;
        if nested.is_some() && outer.is_some_and(|value| normalized(value) != Some(right)) {
            return None;
        }
        Some(right)
    });
    Some(Observation { id, right })
}

#[derive(Clone, PartialEq, Eq)]
struct Scope {
    keyword: String,
    correct: bool,
    channel: Option<crate::catalog::search::results::SearchChannel>,
}

struct Episode {
    id: Uuid,
    token: SessionToken,
    api: Weak<dyn Api>,
    credentials: String,
    scope: Scope,
    started: Instant,
    next_offset: Option<usize>,
    seen: HashMap<String, Option<Rights>>,
    rows: HashMap<Uuid, TrackView>,
}

impl Episode {
    fn live(&self) -> bool {
        self.started.elapsed() < LIFETIME
    }

    fn owns(&self, token: &SessionToken, api: &Weak<dyn Api>, credentials: &str) -> bool {
        self.live()
            && &self.token == token
            && self.api.ptr_eq(api)
            && self.credentials == credentials
    }
}

pub(super) struct Read {
    episode: Uuid,
    offset: usize,
    credentials: String,
}

pub(in crate::client) struct LyricGroupSelection {
    episode: Uuid,
    receipts: Vec<Uuid>,
    pub(in crate::client) anchor_id: String,
    pub(in crate::client) tracks: Vec<TrackView>,
}

#[derive(Default)]
pub(in crate::client) struct LyricGroupPlayback(Mutex<Option<Episode>>);

impl LyricGroupPlayback {
    pub(super) fn begin(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        query: &SearchResultQuery,
        credentials: Option<String>,
    ) -> Result<Option<Read>> {
        if token.uid.is_none() || query.category != SearchCategory::Lyrics {
            return Ok(None);
        }
        let scope = Scope {
            keyword: crate::catalog::search::results::validate_query(query)?,
            correct: query.correct,
            channel: query.channel,
        };
        let api = Arc::downgrade(api);
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if query.offset == 0 {
            *state = credentials.as_ref().map(|credentials| Episode {
                id: Uuid::new_v4(),
                token: token.clone(),
                api: api.clone(),
                credentials: credentials.clone(),
                scope: scope.clone(),
                started: Instant::now(),
                next_offset: Some(0),
                seen: HashMap::new(),
                rows: HashMap::new(),
            });
        }
        let Some(episode) = state.as_ref().filter(|episode| {
            episode.scope == scope
                && episode.next_offset == Some(query.offset)
                && credentials
                    .as_deref()
                    .is_some_and(|revision| episode.owns(token, &api, revision))
        }) else {
            return Ok(None);
        };
        Ok(Some(Read {
            episode: episode.id,
            offset: query.offset,
            credentials: episode.credentials.clone(),
        }))
    }

    pub(super) fn capture(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        read: Read,
        view: &mut SearchResultView,
    ) -> Result<()> {
        let observed = std::mem::take(&mut view.lyric_group_rows);
        let [section] = view.sections.as_mut_slice() else {
            return Ok(());
        };
        if view.query.category != SearchCategory::Lyrics
            || section.category != Some(SearchCategory::Lyrics)
            || view.query.offset != read.offset
            || api
                .lyric_group_playback_credential_revision()
                .ok()
                .flatten()
                .as_deref()
                != Some(read.credentials.as_str())
        {
            return Ok(());
        }
        let api = Arc::downgrade(api);
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let Some(episode) = state.as_mut().filter(|episode| {
            episode.id == read.episode
                && episode.next_offset == Some(read.offset)
                && episode.owns(token, &api, &read.credentials)
        }) else {
            return Ok(());
        };
        if episode.seen.len() + observed.len() > MAX_OBSERVED {
            *state = None;
            return Ok(());
        }
        episode.next_offset =
            (view.more && view.next_offset > read.offset).then_some(view.next_offset);
        for Observation { id, right } in observed {
            episode
                .seen
                .entry(id)
                .and_modify(|previous| {
                    if *previous != right {
                        *previous = None;
                    }
                })
                .or_insert(right);
        }
        // Late pages can reveal a contradictory duplicate. Retire every old
        // receipt for that ID, including any being used by a held action.
        let seen = &episode.seen;
        episode
            .rows
            .retain(|_, row| seen.get(&row.id).is_some_and(Option::is_some));
        for item in &mut section.items {
            let SearchItem::Song {
                track,
                lyric_group_playback,
                ..
            } = item
            else {
                continue;
            };
            if let Some(Some(right)) = episode.seen.get(&track.id) {
                let receipt = Uuid::new_v4();
                episode.rows.insert(receipt, track.clone());
                *lyric_group_playback = Some(Box::new(LyricGroupPlaybackRow {
                    receipt: receipt.simple().to_string(),
                    right: right.kind,
                }));
            }
        }
        Ok(())
    }

    pub(in crate::client) fn resolve(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: &str,
        anchor: &str,
        rows: &[String],
    ) -> Result<LyricGroupSelection> {
        const INVALID: Error = Error::InvalidInput("invalid lyric search playback selection");
        let parse = |value: &str| {
            if value.len() != 32
                || !value
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err(INVALID);
            }
            Uuid::try_parse(value).map_err(|_| INVALID)
        };
        if !(2..=MAX_GROUP).contains(&rows.len()) || !rows.iter().any(|receipt| receipt == anchor) {
            return Err(INVALID);
        }
        let anchor = parse(anchor)?;
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let episode = state
            .as_ref()
            .filter(|episode| episode.owns(token, &Arc::downgrade(api), credentials))
            .ok_or(Error::StaleResponse)?;
        let mut seen = HashSet::new();
        let mut receipts = Vec::with_capacity(rows.len());
        let mut tracks = Vec::with_capacity(rows.len());
        let mut anchor_id = None;
        for receipt in rows {
            let receipt = parse(receipt)?;
            let track = episode.rows.get(&receipt).ok_or(Error::StaleResponse)?;
            if !seen.insert(&track.id) {
                return Err(INVALID);
            }
            let right = episode
                .seen
                .get(&track.id)
                .and_then(Option::as_ref)
                .ok_or(Error::StaleResponse)?;
            receipts.push(receipt);
            match right.kind {
                SearchNextRight::Playable => {
                    if receipt == anchor {
                        anchor_id = Some(track.id.clone());
                    }
                    tracks.push(track.clone());
                }
                SearchNextRight::CopyrightBlocked if receipt != anchor => {}
                SearchNextRight::CopyrightBlocked => {
                    return Err(Error::State("selected search song is unavailable online"));
                }
            }
        }
        Ok(LyricGroupSelection {
            episode: episode.id,
            receipts,
            anchor_id: anchor_id.ok_or(INVALID)?,
            tracks,
        })
    }

    pub(in crate::client) fn current(&self, selection: &LyricGroupSelection) -> bool {
        self.0.lock().is_ok_and(|state| {
            state.as_ref().is_some_and(|episode| {
                episode.live()
                    && episode.id == selection.episode
                    && selection
                        .receipts
                        .iter()
                        .all(|row| episode.rows.contains_key(row))
            })
        })
    }
}

#[cfg(test)]
mod tests;
