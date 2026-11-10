//! Receipts for rows in a native song-tab result, including appended pages.
use crate::catalog::search::results::{SearchChannel, SearchResultQuery};
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
use serde_json::json;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

const MAX_PAGES: usize = 192;
const MAX_ROWS: usize = 500;
const LIFETIME: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SearchDropRow {
    pub receipt: String,
    pub track_id: String,
}

/// Closed ordinary online rights subset. Unknown/trial/alternate rows cannot
/// silently become a partial ordinary menu action.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchNextRight {
    Playable,
    CopyrightBlocked,
}

impl SearchNextRight {
    pub(super) fn from_song(song: &serde_json::Value, track_id: &str) -> Option<Self> {
        let privilege = &song["privilege"];
        if !privilege.is_object()
            || (!privilege["id"].is_null()
                && identifier(&privilege["id"]).ok().as_deref() != Some(track_id))
            || [
                "programId",
                "isOfflineDownOnline",
                "audio",
                "radio",
                "offlineId",
            ]
            .iter()
            .any(|key| js_truthy(&song[*key]))
        {
            return None;
        }
        let status = super::privilege::status(&privilege["st"])?;
        let bitrate = super::privilege::status(&privilege["pl"])?;
        let flags = match &privilege["flag"] {
            serde_json::Value::Null => 0,
            value => value
                .as_i64()
                .or_else(|| value.as_str()?.trim().parse().ok())?,
        };
        if flags & (1 << 10) != 0 {
            return None;
        }
        // M35.j distinguishes negative status from fee. M23.Z admits
        // positive maxPlayBr; M59 substitutes a same-recording backup before
        // insertion. This lane does not take that alternate branch.
        if status >= 0.0 && bitrate > 0.0 {
            Some(Self::Playable)
        } else if status == -200.0
            && bitrate == 0.0
            && !js_truthy(&song["noCopyrightRcmd"])
            && !js_truthy(&song["noCopyrightRcmds"])
        {
            Some(Self::CopyrightBlocked)
        } else {
            None
        }
    }
}

#[derive(PartialEq, Eq)]
struct GroupScope {
    keyword: String,
    correct: bool,
    channel: Option<SearchChannel>,
}

pub(super) struct GroupAuthority {
    api: Weak<dyn Api>,
    credentials: String,
    scope: GroupScope,
}

impl GroupAuthority {
    pub(super) fn new(query: &SearchResultQuery, api: &Arc<dyn Api>, credentials: String) -> Self {
        Self {
            api: Arc::downgrade(api),
            credentials,
            scope: GroupScope {
                keyword: query.keyword.clone(),
                correct: query.correct,
                channel: query.channel,
            },
        }
    }
}

struct Row {
    track: TrackView,
    next_right: Option<SearchNextRight>,
}

struct Page {
    receipt: String,
    epoch: SessionToken,
    issued: Instant,
    tracks: HashMap<String, Row>,
    group: Option<GroupAuthority>,
}

#[derive(Default)]
pub(in crate::client) struct SearchDropPages(Mutex<VecDeque<Arc<Page>>>);

pub(in crate::client) struct SearchNextSelection {
    pub(in crate::client) tracks: Vec<TrackView>,
    pages: Vec<Arc<Page>>,
}

impl SearchDropPages {
    pub(super) fn capture(
        &self,
        token: &SessionToken,
        tracks: Vec<(TrackView, Option<SearchNextRight>)>,
        group: Option<GroupAuthority>,
    ) -> Result<Option<String>> {
        let tracks: HashMap<_, _> = tracks
            .into_iter()
            .map(|(track, next_right)| (track.id.clone(), Row { track, next_right }))
            .collect();
        if tracks.is_empty() {
            return Ok(None);
        }
        let mut pages = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        pages.retain(|page| &page.epoch == token && page.issued.elapsed() < LIFETIME);
        if pages.len() == MAX_PAGES {
            pages.pop_front();
        }
        let receipt = uuid::Uuid::new_v4().simple().to_string();
        pages.push_back(Arc::new(Page {
            receipt: receipt.clone(),
            epoch: token.clone(),
            issued: Instant::now(),
            tracks,
            group,
        }));
        Ok(Some(receipt))
    }

    pub(crate) fn resolve(
        &self,
        token: &SessionToken,
        rows: &[SearchDropRow],
    ) -> Result<Vec<TrackView>> {
        if rows.is_empty() || rows.len() > MAX_ROWS {
            return Err(Error::InvalidInput("invalid search drop selection size"));
        }
        let pages = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        rows.iter()
            .map(|row| {
                if row.receipt.len() != 32 || !row.receipt.bytes().all(|c| c.is_ascii_hexdigit()) {
                    return Err(Error::InvalidInput("invalid search drop receipt"));
                }
                let id = identifier(&json!(row.track_id))?;
                let page = pages
                    .iter()
                    .find(|page| {
                        page.receipt == row.receipt
                            && &page.epoch == token
                            && page.issued.elapsed() < LIFETIME
                    })
                    .ok_or(Error::State("search drop results expired"))?;
                page.tracks
                    .get(&id)
                    .map(|row| row.track.clone())
                    .ok_or(Error::InvalidInput(
                        "search drop row is outside the observed results",
                    ))
            })
            .collect()
    }

    pub(in crate::client) fn resolve_group_next(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: &str,
        anchor: &SearchDropRow,
        rows: &[SearchDropRow],
    ) -> Result<SearchNextSelection> {
        const INVALID: Error = Error::InvalidInput("invalid search next selection");
        if !(2..=MAX_ROWS).contains(&rows.len()) || !rows.contains(anchor) {
            return Err(INVALID);
        }
        let cache = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let active_api = Arc::downgrade(api);
        let mut used_pages: Vec<Arc<Page>> = Vec::new();
        let mut seen_ids = HashSet::new();
        let mut tracks = Vec::new();
        for target in rows {
            if target.receipt.len() != 32
                || !target.receipt.bytes().all(|byte| byte.is_ascii_hexdigit())
                || !seen_ids.insert(&target.track_id)
            {
                return Err(INVALID);
            }
            let page = cache
                .iter()
                .find(|page| {
                    page.receipt == target.receipt
                        && &page.epoch == token
                        && page.issued.elapsed() < LIFETIME
                        && page.group.as_ref().is_some_and(|source| {
                            source.credentials == credentials && source.api.ptr_eq(&active_api)
                        })
                })
                .ok_or(Error::StaleResponse)?;
            if used_pages.first().is_some_and(|first| {
                first.group.as_ref().map(|source| &source.scope)
                    != page.group.as_ref().map(|source| &source.scope)
            }) {
                return Err(INVALID);
            }
            if !used_pages.iter().any(|seen| Arc::ptr_eq(seen, page)) {
                used_pages.push(page.clone());
            }
            let row = page.tracks.get(&target.track_id).ok_or(INVALID)?;
            match row.next_right {
                Some(SearchNextRight::Playable) => tracks.push(row.track.clone()),
                Some(SearchNextRight::CopyrightBlocked) if target != anchor => {}
                Some(SearchNextRight::CopyrightBlocked) => {
                    return Err(Error::State("selected search song is unavailable online"));
                }
                None => {
                    return Err(Error::State(
                        "search selection has unverified playback rights",
                    ));
                }
            }
        }
        Ok(SearchNextSelection {
            tracks,
            pages: used_pages,
        })
    }

    pub(in crate::client) fn group_next_current(&self, selection: &SearchNextSelection) -> bool {
        self.0.lock().is_ok_and(|cache| {
            selection.pages.iter().all(|page| {
                page.issued.elapsed() < LIFETIME
                    && cache.iter().any(|current| Arc::ptr_eq(current, page))
            })
        })
    }
}

pub(super) fn bounded_track(track: &TrackView) -> bool {
    track.name.len() <= 1024
        && track.album.len() <= 1024
        && track.cover_url.as_ref().is_none_or(|v| v.len() <= 2048)
        && track.artist_refs.len() <= 16
        && track.artist_refs.iter().all(|a| a.name.len() <= 256)
}
