//! Full, account-bound ordinary playlist projection for M/2997 and M/470.
use super::{
    MusicClient,
    dto::{AccountData, PageRequest, TrackPage, TrackView},
};
use crate::{
    Error, Result, account::SessionToken, playlist::PlaylistLocalStore, protocol::api::identifier,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    cmp::Reverse,
    sync::{Arc, Mutex},
};

const LIMIT: usize = 50_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistDurationOrder {
    Ascending,
    Descending,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaylistDurationScope {
    pub receipt: String,
    pub order: PlaylistDurationOrder,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PlaylistDurationSource {
    pub revision: String,
    pub local_revision: Option<String>,
    pub cached_at_ms: Option<u64>,
}

impl PlaylistDurationSource {
    fn from_page(page: &TrackPage) -> Result<Self> {
        Ok(Self {
            revision: page.revision.clone().ok_or(Error::QueueChanged)?,
            local_revision: page
                .playlist_local
                .as_ref()
                .map(|local| local.revision.clone()),
            cached_at_ms: page
                .playlist_offline
                .as_ref()
                .map(|cache| cache.cached_at_ms),
        })
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistDurationPage {
    pub receipt: String,
    pub page: TrackPage,
    pub ascending: Vec<usize>,
    pub descending: Vec<usize>,
}

pub(super) struct Prepared {
    pub token: SessionToken,
    pub source: PlaylistDurationSource,
    pub generation: Option<u64>,
    pub projection: PlaylistDurationPage,
}

impl Prepared {
    pub fn ordinals(&self, order: PlaylistDurationOrder) -> &[usize] {
        match order {
            PlaylistDurationOrder::Ascending => &self.projection.ascending,
            PlaylistDurationOrder::Descending => &self.projection.descending,
        }
    }
}

#[derive(Default)]
struct State {
    generation: u64,
    active: Option<Arc<Prepared>>,
    request: Option<(SessionToken, String)>,
    prefix: Option<(SessionToken, TrackPage)>,
}

#[derive(Clone, Default)]
pub(super) struct Host(Arc<Mutex<State>>);

impl Host {
    pub fn observe(&self, token: &SessionToken, page: &TrackPage) {
        let Some(playlist) = &page.playlist else {
            return;
        };
        let id = playlist.id.as_str();
        let Ok(source) = PlaylistDurationSource::from_page(page) else {
            return;
        };
        let Ok(mut state) = self.0.lock() else {
            return;
        };
        if page.offset == 0 {
            Self::forget_state(&mut state, token, id);
            if page.total <= LIMIT && page.playlist_offline.is_none() && !playlist.favorite {
                state.prefix = Some((token.clone(), page.clone()));
            }
        } else if page.playlist_offline.is_none()
            && let Some((owner, before)) = state.prefix.as_mut()
            && owner == token
            && before.playlist.as_ref().is_some_and(|p| p.id == id)
            && before.tracks.len() == page.offset
            && before.total == page.total
            && PlaylistDurationSource::from_page(before).is_ok_and(|before| before == source)
        {
            merge_suffix(before, page.clone());
        }
    }

    pub fn forget(&self, token: &SessionToken, id: &str) {
        if let Ok(mut state) = self.0.lock() {
            Self::forget_state(&mut state, token, id);
        }
    }

    fn forget_state(state: &mut State, token: &SessionToken, id: &str) {
        let same_playlist = |page: &TrackPage| page.playlist.as_ref().is_some_and(|p| p.id == id);
        if state
            .active
            .as_ref()
            .is_some_and(|v| v.token != *token || same_playlist(&v.projection.page))
        {
            state.active = None;
        }
        if state
            .request
            .as_ref()
            .is_some_and(|(owner, pending)| owner != token || pending == id)
        {
            state.request = None;
        }
        if state
            .prefix
            .as_ref()
            .is_some_and(|(owner, page)| owner != token || same_playlist(page))
        {
            state.prefix = None;
        }
    }

    fn begin(
        &self,
        token: &SessionToken,
        id: &str,
        source: &PlaylistDurationSource,
    ) -> Result<(u64, Option<TrackPage>)> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or(Error::State("playlist duration generation exhausted"))?;
        let matches = |owner: &SessionToken, page: &TrackPage| {
            owner == token
                && page.playlist.as_ref().is_some_and(|p| p.id == id)
                && PlaylistDurationSource::from_page(page).is_ok_and(|observed| &observed == source)
        };
        let before = state
            .active
            .as_ref()
            .filter(|active| {
                source.cached_at_ms.is_none() && matches(&active.token, &active.projection.page)
            })
            .map(|active| &active.projection.page)
            .or_else(|| {
                state
                    .prefix
                    .as_ref()
                    .filter(|(owner, page)| matches(owner, page))
                    .map(|(_, page)| page)
            })
            .cloned();
        state.active = None;
        state.request = Some((token.clone(), id.to_owned()));
        Ok((state.generation, before))
    }

    fn publish(&self, sequence: u64, value: Arc<Prepared>) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.generation != sequence || state.request.take().is_none() {
            return Err(Error::StaleResponse);
        }
        if state.prefix.as_ref().is_some_and(|(owner, before)| {
            owner == &value.token
                && before.playlist.as_ref().map(|p| &p.id)
                    == value.projection.page.playlist.as_ref().map(|p| &p.id)
        }) {
            state.prefix = None;
        }
        state.active = Some(value);
        Ok(())
    }

    pub fn get(&self, token: &SessionToken, receipt: &str) -> Result<Arc<Prepared>> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        state
            .active
            .as_ref()
            .filter(|p| p.token == *token && p.projection.receipt == receipt)
            .cloned()
            .ok_or(Error::QueueChanged)
    }

    pub fn current(&self, value: &Arc<Prepared>) -> bool {
        self.0
            .lock()
            .is_ok_and(|s| s.active.as_ref().is_some_and(|p| Arc::ptr_eq(p, value)))
    }
}

fn merge_suffix(before: &mut TrackPage, suffix: TrackPage) {
    before.tracks.extend(suffix.tracks);
    before.more = suffix.more;
    if let (Some(before), Some(suffix)) = (&mut before.playlist_local, suffix.playlist_local) {
        before.rows.extend(suffix.rows);
    }
}

fn ordered(tracks: &[TrackView], order: PlaylistDurationOrder) -> Vec<usize> {
    let duration = |i: usize| {
        let v = tracks[i].duration_ms;
        if v <= 9_007_199_254_740_991 { v } else { 0 }
    };
    let mut result = (0..tracks.len()).collect::<Vec<_>>();
    match order {
        PlaylistDurationOrder::Ascending => result.sort_by_key(|&i| duration(i)),
        PlaylistDurationOrder::Descending => result.sort_by_key(|&i| Reverse(duration(i))),
    }
    result
}

impl MusicClient {
    pub async fn playlist_duration_prepare(
        &self,
        epoch: &str,
        id: &str,
        source: PlaylistDurationSource,
    ) -> Result<AccountData<PlaylistDurationPage>> {
        let id = identifier(&json!(id))?;
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        let (sequence, prefix) = self.playlist_duration.begin(&token, &id, &source)?;
        let membership = PlaylistLocalStore::new(self.session.clone())?;
        let generation = if source.local_revision.is_some() {
            membership.generation(&token, &id)?
        } else {
            None
        };
        let offset = prefix.as_ref().map_or(0, |page| page.tracks.len());
        let page = PageRequest {
            offset,
            limit: LIMIT - offset,
            revision: Some(source.revision.clone()),
        };
        let value = match prefix {
            Some(before) if !before.more && before.tracks.len() == before.total => before,
            before => {
                let fetched = if source.cached_at_ms.is_some() {
                    self.offline_playlist_page(&token, &id, &page, source.local_revision.as_deref())
                        .await?
                        .ok_or(Error::QueueChanged)?
                } else {
                    self.online_playlist_page(
                        &token,
                        &id,
                        &page,
                        source.local_revision.as_deref(),
                        Some(LIMIT),
                    )
                    .await?
                };
                if let Some(mut before) = before {
                    if fetched.offset != offset
                        || fetched.total != before.total
                        || PlaylistDurationSource::from_page(&fetched)? != source
                    {
                        return Err(Error::QueueChanged);
                    }
                    merge_suffix(&mut before, fetched);
                    before
                } else {
                    fetched
                }
            }
        };
        let playlist = value.playlist.as_ref().ok_or(Error::QueueChanged)?;
        if playlist.id != id
            || playlist.favorite
            || value.total > LIMIT
            || value.more
            || value.offset != 0
            || value.tracks.len() != value.total
            || source != PlaylistDurationSource::from_page(&value)?
            || (source.local_revision.is_some()
                && membership.generation(&token, &id)? != generation)
        {
            return Err(Error::QueueChanged);
        }
        let projection = PlaylistDurationPage {
            receipt: uuid::Uuid::new_v4().simple().to_string(),
            ascending: ordered(&value.tracks, PlaylistDurationOrder::Ascending),
            descending: ordered(&value.tracks, PlaylistDurationOrder::Descending),
            page: value,
        };
        let prepared = Arc::new(Prepared {
            token: token.clone(),
            source,
            generation,
            projection: projection.clone(),
        });
        self.session.with_token(&token, |_| {
            self.playlist_duration.publish(sequence, prepared)?;
            Ok(AccountData {
                epoch: epoch.to_owned(),
                value: projection,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_official_duration_order_keeps_ties_and_zero() {
        // M/470 + M/177 executed from recovered 3.1.42.205594, in original order.
        let rows: Vec<TrackView> = [53_000, 9_000, 9_000, 0, 2_000]
            .into_iter()
            .enumerate()
            .map(|(index, duration_ms)| {
                let mut row = super::super::content::missing_playlist_row(index.to_string(), false);
                row.duration_ms = duration_ms;
                row
            })
            .collect();
        assert_eq!(
            ordered(&rows, PlaylistDurationOrder::Ascending),
            [3, 4, 1, 2, 0]
        );
        assert_eq!(
            ordered(&rows, PlaylistDurationOrder::Descending),
            [0, 1, 2, 4, 3]
        );
    }
}
