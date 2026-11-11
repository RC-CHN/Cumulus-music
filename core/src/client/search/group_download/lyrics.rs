//! Dedicated lyric result evidence for downloads. Playback and collection
//! receipts never identify a download candidate.
use super::*;
use crate::client::search::{SearchCategory, SearchItem, SearchResultView};
use uuid::Uuid;

const MAX_LYRIC_RESULTS: usize = 10_100;
const INVALID: Error = Error::InvalidInput("invalid lyric download selection");

pub(in crate::client::search) struct LyricDownloadObservation {
    id: String,
    index: Option<usize>,
    rights: Option<LyricRights>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct LyricRights {
    row: RowRights,
    singleton_anchor_payment: Option<bool>,
}

pub(in crate::client::search) fn observe(
    hit: &Value,
    projected: Option<(&crate::client::dto::TrackView, usize)>,
) -> Option<LyricDownloadObservation> {
    let song = hit.get("song")?;
    let id = identifier(&song["id"]).ok()?;
    if id.len() > 128 || id.starts_with('0') {
        return None;
    }
    let rights = projected.and_then(|(track, _)| {
        if !super::super::results::lyric_destination_song(hit, song, track, &id) {
            return None;
        }
        let normalize = |privilege: &Value| {
            let mut song = song.clone();
            song["privilege"] = privilege.clone();
            // The dedicated Lyrics row can carry a known paid/member offer;
            // acquisition still requires positive entitlement at the native URL.
            Some(LyricRights {
                row: RowRights::observe_with_source(&song, &id, true)?,
                singleton_anchor_payment: singleton_anchor_payment(privilege, &id),
            })
        };
        let outer = hit.get("privilege");
        let nested = song.get("privilege");
        let rights = normalize(nested.or(outer)?)?;
        if nested.is_some() && outer.is_some_and(|value| normalize(value) != Some(rights)) {
            return None;
        }
        Some(rights)
    });
    Some(LyricDownloadObservation {
        id,
        index: projected.map(|(_, index)| index),
        rights,
    })
}

pub(super) struct Visit {
    id: Uuid,
    owner: SessionToken,
    api: Weak<dyn Api>,
    revision: String,
    scope: QueryScope,
    started: Instant,
    next: Option<usize>,
    rows: HashMap<Uuid, String>,
    rights: HashMap<String, Option<LyricRights>>,
}

impl Visit {
    fn live(&self) -> bool {
        self.started.elapsed() < PAGE_LIFETIME
    }

    fn owned(&self, owner: &SessionToken, api: &Weak<dyn Api>, revision: &str) -> bool {
        self.live() && self.owner == *owner && self.api.ptr_eq(api) && self.revision == revision
    }

    fn row(&self, target: &SearchGroupDownloadTarget) -> Result<LyricRights> {
        let receipt = target.receipt.parse::<Uuid>().map_err(|_| INVALID)?;
        if receipt.simple().to_string() != target.receipt
            || self.rows.get(&receipt) != Some(&target.track_id)
        {
            return Err(Error::StaleResponse);
        }
        self.rights
            .get(&target.track_id)
            .copied()
            .flatten()
            .ok_or(Error::StaleResponse)
    }
}

pub(in crate::client::search) struct Read {
    id: Uuid,
    offset: usize,
    revision: String,
}

pub(super) struct Selection {
    id: Uuid,
    targets: Vec<SearchGroupDownloadTarget>,
    pub(super) singleton_anchor_supported: bool,
}

impl Selection {
    pub(super) fn current(&self, visit: Option<&Visit>) -> bool {
        visit.is_some_and(|visit| {
            visit.live()
                && visit.id == self.id
                && self.targets.iter().all(|target| visit.row(target).is_ok())
        })
    }
}

impl SearchGroupDownloads {
    pub(in crate::client::search) fn begin_lyric_read(
        &self,
        owner: &SessionToken,
        api: &Arc<dyn Api>,
        query: &SearchResultQuery,
        revision: Option<String>,
    ) -> Result<Option<Read>> {
        if owner.uid.is_none() || query.category != SearchCategory::Lyrics {
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
            state.lyrics = revision.as_ref().map(|revision| Visit {
                id: Uuid::new_v4(),
                owner: owner.clone(),
                api: api.clone(),
                revision: revision.clone(),
                scope: scope.clone(),
                started: Instant::now(),
                next: Some(0),
                rows: HashMap::new(),
                rights: HashMap::new(),
            });
        }
        Ok(state
            .lyrics
            .as_ref()
            .filter(|visit| {
                visit.scope == scope
                    && visit.next == Some(query.offset)
                    && revision
                        .as_deref()
                        .is_some_and(|revision| visit.owned(owner, &api, revision))
            })
            .map(|visit| Read {
                id: visit.id,
                offset: query.offset,
                revision: visit.revision.clone(),
            }))
    }

    pub(in crate::client::search) fn capture_lyrics(
        &self,
        owner: &SessionToken,
        api: &Arc<dyn Api>,
        read: Read,
        view: &mut SearchResultView,
    ) -> Result<()> {
        let observations = std::mem::take(&mut view.lyric_download_rows);
        let [section] = view.sections.as_mut_slice() else {
            return Ok(());
        };
        if view.query.category != SearchCategory::Lyrics
            || section.category != Some(SearchCategory::Lyrics)
            || view.query.offset != read.offset
            || api
                .lyric_group_download_credential_revision()
                .ok()
                .flatten()
                .as_deref()
                != Some(&read.revision)
        {
            return Ok(());
        }
        let api = Arc::downgrade(api);
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let Some(visit) = state.lyrics.as_mut().filter(|visit| {
            visit.id == read.id
                && visit.next == Some(read.offset)
                && visit.owned(owner, &api, &read.revision)
        }) else {
            return Ok(());
        };
        if visit.rows.len() + observations.len() > MAX_LYRIC_RESULTS
            || visit.rights.len() + observations.len() > MAX_LYRIC_RESULTS
        {
            state.lyrics = None;
            return Ok(());
        }
        visit.next = (view.more && view.next_offset > read.offset).then_some(view.next_offset);
        for candidate in &observations {
            visit
                .rights
                .entry(candidate.id.clone())
                .and_modify(|rights| {
                    if *rights != candidate.rights {
                        *rights = None;
                    }
                })
                .or_insert(candidate.rights);
        }
        let rights = &visit.rights;
        visit
            .rows
            .retain(|_, id| rights.get(id).is_some_and(Option::is_some));
        for candidate in observations {
            let Some(index) = candidate.index else {
                continue;
            };
            let Some(rights) = visit.rights.get(&candidate.id).copied().flatten() else {
                continue;
            };
            if candidate.rights != Some(rights) {
                continue;
            }
            if let Some(SearchItem::Song {
                track,
                group_download,
                ..
            }) = section.items.get_mut(index)
                && track.id == candidate.id
            {
                let receipt = Uuid::new_v4();
                visit.rows.insert(receipt, candidate.id);
                *group_download = Some(Box::new(rights.row.row(receipt.simple().to_string())));
            }
        }
        Ok(())
    }

    pub(super) fn begin_lyric_selection(
        &self,
        owner: SessionToken,
        api: Arc<dyn Api>,
        revision: String,
        anchor: SearchGroupDownloadTarget,
        targets: Vec<SearchGroupDownloadTarget>,
    ) -> Result<(u64, Arc<Observed>)> {
        if !(2..=MAX_ROWS).contains(&targets.len()) {
            return Err(INVALID);
        }
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let visit = state
            .lyrics
            .as_ref()
            .filter(|visit| visit.owned(&owner, &Arc::downgrade(&api), &revision))
            .ok_or(Error::StaleResponse)?;
        let singleton_anchor_supported = visit.row(&anchor)?.singleton_anchor_payment == Some(true);
        let id = visit.id;
        let mut seen = BTreeSet::new();
        let rows = targets
            .iter()
            .map(|target| {
                if !seen.insert(target.track_id.as_str())
                    || (target.track_id == anchor.track_id && *target != anchor)
                {
                    return Err(INVALID);
                }
                Ok((target.track_id.clone(), visit.row(target)?.row))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut targets = targets;
        if !targets.contains(&anchor) {
            targets.push(anchor.clone());
        }
        let ticket = Self::begin_plan(&mut state)?;
        Ok((
            ticket,
            Arc::new(Observed {
                owner,
                api,
                revision,
                anchor: anchor.track_id,
                rows,
                source: Source::Lyrics(Selection {
                    id,
                    targets,
                    singleton_anchor_supported,
                }),
            }),
        ))
    }

    fn lyric_anchor(
        &self,
        owner: &SessionToken,
        api: &Arc<dyn Api>,
        revision: &str,
        anchor: &SearchGroupDownloadTarget,
    ) -> Result<Uuid> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let visit = state
            .lyrics
            .as_ref()
            .filter(|visit| visit.owned(owner, &Arc::downgrade(api), revision))
            .ok_or(Error::StaleResponse)?;
        visit.row(anchor)?;
        Ok(visit.id)
    }
}

impl MusicClient {
    pub(super) async fn preview_lyric_download_anchor(
        &self,
        epoch: &str,
        owner: SessionToken,
        anchor: SearchGroupDownloadTarget,
    ) -> Result<SearchGroupDownloadReply> {
        let login = self.auth.pending_guard();
        login()?;
        let api = self.session.native_api(&owner)?;
        let revision = api
            .lyric_group_download_credential_revision()?
            .ok_or(Error::StaleResponse)?;
        let host = &self.search.group_downloads;
        let visit = host.lyric_anchor(&owner, &api, &revision, &anchor)?;
        let local = self.local.context().await?;
        let session = self.session.clone();
        let token = owner.clone();
        let id = anchor.track_id.clone();
        let show = tokio::task::spawn_blocking(move || {
            let ids = BTreeSet::from([id.as_str()]);
            let matched = local
                .library
                .matched_online_song_ids(&session, &token, &ids)?;
            if matched.contains(id.as_str()) {
                return Ok(false);
            }
            session.with_token(&token, |uid| {
                session.persistent_store()?.read_snapshot(|tx| {
                    Ok(!tx
                        .playback_download_rows(uid, &id, PlaybackDownloadKind::Song)?
                        .iter()
                        .any(|row| row.status == "complete"))
                })
            })
        })
        .await
        .map_err(|_| Error::State("lyric download menu lookup stopped"))??;
        login()?;
        self.session.require_api(&owner, &api)?;
        if api.lyric_group_download_credential_revision()?.as_deref() != Some(&revision)
            || host.lyric_anchor(&owner, &api, &revision, &anchor)? != visit
        {
            return Err(Error::StaleResponse);
        }
        Ok(SearchGroupDownloadReply::LyricMenu {
            epoch: epoch.into(),
            show,
        })
    }
}
