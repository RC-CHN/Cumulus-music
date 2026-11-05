//! Receipts for rows in a native song-tab result, including appended pages.
use crate::{
    Error, Result, account::SessionToken, client::dto::TrackView, protocol::api::identifier,
};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

const MAX_PAGES: usize = 192;
const MAX_ROWS: usize = 500;
const LIFETIME: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchDropRow {
    pub receipt: String,
    pub track_id: String,
}

struct Page {
    receipt: String,
    epoch: SessionToken,
    issued: Instant,
    tracks: HashMap<String, TrackView>,
}

#[derive(Default)]
pub(super) struct SearchDropPages(Mutex<VecDeque<Page>>);

impl SearchDropPages {
    pub(super) fn capture(
        &self,
        token: &SessionToken,
        tracks: Vec<TrackView>,
    ) -> Result<Option<String>> {
        let tracks: HashMap<_, _> = tracks
            .into_iter()
            .map(|track| (track.id.clone(), track))
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
        pages.push_back(Page {
            receipt: receipt.clone(),
            epoch: token.clone(),
            issued: Instant::now(),
            tracks,
        });
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
                page.tracks.get(&id).cloned().ok_or(Error::InvalidInput(
                    "search drop row is outside the observed results",
                ))
            })
            .collect()
    }
}

pub(super) fn bounded_track(track: &TrackView) -> bool {
    track.name.len() <= 1024
        && track.album.len() <= 1024
        && track.cover_url.as_ref().is_none_or(|v| v.len() <= 2048)
        && track.artist_refs.len() <= 16
        && track.artist_refs.iter().all(|a| a.name.len() <= 256)
}
