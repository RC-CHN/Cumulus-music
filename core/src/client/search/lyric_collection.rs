//! Lyrics selection receipts authorize only an explicitly chosen collection destination.
use super::{SearchCategory, SearchItem, SearchResultQuery, SearchResultView};
use crate::{
    Error, Result,
    account::SessionToken,
    client::dto::TrackView,
    protocol::{
        api::{Api, identifier},
        wire::serialization::js_truthy,
    },
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use uuid::Uuid;

const MAX_SELECTION: usize = 500;
const MAX_OBSERVED: usize = 10_100;
const LIFETIME: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Privilege {
    Undefined,
    Positive,
    Falsey,
    Denied,
    Unknown,
}

impl Privilege {
    fn preferred(self) -> bool {
        matches!(self, Self::Undefined | Self::Positive)
    }

    fn allowed(self) -> bool {
        self.preferred() || self == Self::Falsey
    }
}

fn privilege(row: &Value, id: &str) -> Privilege {
    // M21.W leaves the normalized privilege absent when raw privilege
    // itself is absent or null. An invalid asserted object is not absence.
    if row.is_null() {
        return Privilege::Undefined;
    }
    let Some(row) = row.as_object() else {
        return Privilege::Unknown;
    };
    if row
        .get("id")
        .filter(|value| !value.is_null())
        .is_some_and(|value| identifier(value).ok().as_deref() != Some(id))
    {
        return Privilege::Unknown;
    }
    let raw = row.get("subp");
    let normalized = row.get("subPriv");
    if raw.zip(normalized).is_some_and(|(a, b)| a != b) {
        return Privilege::Unknown;
    }
    match raw.or(normalized) {
        None => Privilege::Undefined,
        Some(value) if !js_truthy(value) => Privilege::Falsey,
        Some(value) => match super::privilege::status(value) {
            Some(n) if n > 0.0 => Privilege::Positive,
            Some(_) => Privilege::Denied,
            None => Privilege::Unknown,
        },
    }
}

pub(super) struct Observation {
    id: String,
    privilege: Option<Privilege>,
}

pub(super) fn observe(raw: &Value, track: Option<&TrackView>) -> Option<Observation> {
    let song = raw.get("song")?;
    let id = identifier(&song["id"]).ok()?;
    if id.len() > 128 || id.starts_with('0') {
        return None;
    }
    let collection = track.and_then(|track| {
        if !super::results::lyric_destination_song(raw, song, track, &id) {
            return None;
        }
        let outer = raw.get("privilege");
        let nested = song.get("privilege");
        // M15.Wn spreads outer then nested Song. Two conflicting explicit
        // observations may not produce a collection write.
        let current = privilege(nested.or(outer).unwrap_or(&Value::Null), &id);
        if nested.is_some() && outer.is_some_and(|value| privilege(value, &id) != current) {
            return Some(Privilege::Unknown);
        }
        Some(current)
    });
    Some(Observation {
        id,
        privilege: collection,
    })
}

#[derive(Clone, PartialEq, Eq)]
struct QueryScope {
    keyword: String,
    correct: bool,
    channel: Option<crate::catalog::search::results::SearchChannel>,
}

struct Episode {
    id: Uuid,
    token: SessionToken,
    api: Weak<dyn Api>,
    revision: String,
    scope: QueryScope,
    started: Instant,
    next_offset: Option<usize>,
    seen: HashMap<String, Option<Privilege>>,
    rows: HashMap<Uuid, String>,
}

impl Episode {
    fn owns(&self, token: &SessionToken, api: &Weak<dyn Api>, revision: &str) -> bool {
        self.started.elapsed() < LIFETIME
            && &self.token == token
            && self.api.ptr_eq(api)
            && self.revision == revision
    }
}

pub(super) struct Read {
    episode: Uuid,
    offset: usize,
    revision: String,
}

pub(in crate::client) struct LyricCollectionSelection {
    episode: Uuid,
    receipts: Vec<Uuid>,
    pub(in crate::client) songs: Vec<String>,
}

#[derive(Default)]
pub(in crate::client) struct LyricCollection(Mutex<Option<Episode>>);

impl LyricCollection {
    pub(super) fn begin(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        query: &SearchResultQuery,
        revision: Option<String>,
    ) -> Result<Option<Read>> {
        if token.uid.is_none() || query.category != SearchCategory::Lyrics {
            return Ok(None);
        }
        let scope = QueryScope {
            keyword: crate::catalog::search::results::validate_query(query)?,
            correct: query.correct,
            channel: query.channel,
        };
        let api = Arc::downgrade(api);
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if query.offset == 0 {
            *state = revision.as_ref().map(|revision| Episode {
                id: Uuid::new_v4(),
                token: token.clone(),
                api: api.clone(),
                revision: revision.clone(),
                scope: scope.clone(),
                started: Instant::now(),
                next_offset: Some(0),
                seen: HashMap::new(),
                rows: HashMap::new(),
            });
        }
        Ok(state
            .as_ref()
            .filter(|episode| {
                episode.scope == scope
                    && episode.next_offset == Some(query.offset)
                    && revision
                        .as_deref()
                        .is_some_and(|revision| episode.owns(token, &api, revision))
            })
            .map(|episode| Read {
                episode: episode.id,
                offset: query.offset,
                revision: episode.revision.clone(),
            }))
    }

    pub(super) fn capture(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        read: Read,
        view: &mut SearchResultView,
    ) -> Result<()> {
        let observed = std::mem::take(&mut view.lyric_group_collection_rows);
        let [section] = view.sections.as_mut_slice() else {
            return Ok(());
        };
        if view.query.category != SearchCategory::Lyrics
            || section.category != Some(SearchCategory::Lyrics)
            || view.query.offset != read.offset
            || api
                .lyric_group_collection_credential_revision()
                .ok()
                .flatten()
                .as_deref()
                != Some(read.revision.as_str())
        {
            return Ok(());
        }
        let api = Arc::downgrade(api);
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let Some(episode) = state.as_mut().filter(|episode| {
            episode.id == read.episode
                && episode.next_offset == Some(read.offset)
                && episode.owns(token, &api, &read.revision)
        }) else {
            return Ok(());
        };
        if episode.seen.len() + observed.len() > MAX_OBSERVED {
            *state = None;
            return Ok(());
        }
        episode.next_offset =
            (view.more && view.next_offset > read.offset).then_some(view.next_offset);
        for Observation { id, privilege } in observed {
            episode
                .seen
                .entry(id)
                .and_modify(|previous| {
                    if *previous != privilege {
                        *previous = None;
                    }
                })
                .or_insert(privilege);
        }
        let seen = &episode.seen;
        episode
            .rows
            .retain(|_, id| seen.get(id).is_some_and(Option::is_some));
        for item in &mut section.items {
            if let SearchItem::Song {
                track,
                lyric_group_collection_receipt,
                ..
            } = item
                && seen.get(&track.id).is_some_and(Option::is_some)
            {
                let receipt = Uuid::new_v4();
                episode.rows.insert(receipt, track.id.clone());
                *lyric_group_collection_receipt = Some(receipt.simple().to_string());
            }
        }
        Ok(())
    }

    pub(in crate::client) fn resolve(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        revision: &str,
        anchor: &str,
        rows: &[String],
    ) -> Result<LyricCollectionSelection> {
        const INVALID: Error = Error::InvalidInput("invalid lyric collection selection");
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
        if !(2..=MAX_SELECTION).contains(&rows.len()) {
            return Err(INVALID);
        }
        let anchor = parse(anchor)?;
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let episode = state
            .as_ref()
            .filter(|episode| episode.owns(token, &Arc::downgrade(api), revision))
            .ok_or(Error::StaleResponse)?;
        // Modified secondary clicks may retain a group outside the clicked
        // row; only the validated ordered selection contributes song IDs.
        if !episode.rows.contains_key(&anchor) {
            return Err(Error::StaleResponse);
        }
        let mut receipts = Vec::with_capacity(rows.len() + 1);
        receipts.push(anchor);
        let mut seen = HashSet::new();
        let mut selected = Vec::with_capacity(rows.len());
        for receipt in rows {
            let receipt = parse(receipt)?;
            let id = episode.rows.get(&receipt).ok_or(Error::StaleResponse)?;
            if !seen.insert(id.as_str()) {
                return Err(INVALID);
            }
            let privilege = episode
                .seen
                .get(id)
                .and_then(|p| *p)
                .ok_or(Error::StaleResponse)?;
            receipts.push(receipt);
            selected.push((id, privilege));
        }
        // M3085 reverses its filtered temporary copy. M3160 reverses both
        // IDs and tracks again before the wire; the survivors retain the
        // original Set order. Only original first can be the fallback.
        let songs = if selected
            .iter()
            .any(|(_, p)| p.preferred() || *p == Privilege::Unknown)
        {
            selected
                .iter()
                .filter(|(_, p)| p.preferred())
                .map(|(id, _)| id.as_str().to_owned())
                .collect()
        } else {
            selected
                .first()
                .filter(|(_, p)| p.allowed())
                .map(|(id, _)| id.as_str().to_owned())
                .into_iter()
                .collect()
        };
        Ok(LyricCollectionSelection {
            episode: episode.id,
            receipts,
            songs,
        })
    }

    pub(in crate::client) fn current(&self, selection: &LyricCollectionSelection) -> bool {
        self.0.lock().is_ok_and(|state| {
            state.as_ref().is_some_and(|episode| {
                episode.started.elapsed() < LIFETIME
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
