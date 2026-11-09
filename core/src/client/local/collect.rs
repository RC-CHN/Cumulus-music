//! LocalMusicBatch Favorite: current mapped catalogue rows and existing owned targets.
use super::{LocalGroup, LocalQuery, LocalTrackReference, MusicClient, track_reference};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    client::dto::{AccountData, PlaylistCollectPage, PlaylistCollectSort},
    library::{LocalLibrary, LocalMapping, MappingSource},
    playlist::{PlaylistCommand, PlaylistPhase, PlaylistService, TrackChange},
    protocol::{
        api::{Api, GuardedPlaylistAdd, identifier},
        wire::serialization::js_truthy,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, atomic::Ordering},
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalCollectSelection {
    query: LocalQuery,
    revision: String,
    tracks: Vec<LocalTrackReference>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCollectPlan {
    pub receipt: String,
    pub count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCollectResult {
    pub phase: PlaylistPhase,
    pub duplicate: bool,
}

const LIFETIME: Duration = Duration::from_secs(300);
const NO_MATCH: Error =
    Error::InvalidInput("every selected local track requires a verified catalog match");

fn catalogue_id(mapping: &LocalMapping) -> Result<String> {
    // M170 and M461 put hydrated privilege on onlineTrack, not on the local
    // row. M3085 checks the latter, so nested subp must not filter this batch.
    if !matches!(
        mapping.source,
        Some(MappingSource::Search | MappingSource::Manual)
    ) {
        return Err(NO_MATCH);
    }
    let song = mapping
        .song
        .as_ref()
        .filter(|song| song.is_object())
        .ok_or(NO_MATCH)?;
    let id = identifier(&song["id"]).map_err(|_| NO_MATCH)?;
    let privilege = &song["privilege"];
    if id != mapping.song_id
        || (!privilege.is_null() && !privilege.is_object())
        || (!privilege["id"].is_null()
            && identifier(&privilege["id"]).ok().as_deref() != Some(id.as_str()))
        || [
            "voice",
            "voice_id",
            "voiceId",
            "djProgramId",
            "_cloud_public_id",
            "_cloud_catalog_id",
        ]
        .into_iter()
        .any(|field| js_truthy(&song[field]))
        || ["cs", "cloudSong"]
            .into_iter()
            .any(|field| js_truthy(&privilege[field]))
    {
        return Err(NO_MATCH);
    }
    Ok(id)
}

fn selection_ids(
    library: &LocalLibrary,
    session: &AccountSession,
    token: &SessionToken,
    selection: &LocalCollectSelection,
) -> Result<Vec<String>> {
    let rows = selection
        .tracks
        .iter()
        .map(|row| (row.id.as_str(), row.revision.as_str()))
        .collect::<Vec<_>>();
    let mut unique = BTreeSet::new();
    library
        .batch_collection_mappings(session, token, &selection.query, &rows)?
        .iter()
        .map(|mapping| {
            let id = catalogue_id(mapping)?;
            if !unique.insert(id.clone()) {
                return Err(Error::InvalidInput(
                    "selected local tracks have duplicate catalog matches",
                ));
            }
            Ok(id)
        })
        .collect()
}

struct Source {
    token: SessionToken,
    session: Arc<AccountSession>,
    api: Arc<dyn Api>,
    credentials: String,
    library: LocalLibrary,
    selection: LocalCollectSelection,
    songs: Vec<String>,
    receipt: String,
    created: Instant,
}
impl Source {
    fn require(&self) -> Result<()> {
        self.session.require_api(&self.token, &self.api)?;
        if self.created.elapsed() >= LIFETIME
            || self.api.local_collection_scope_revision()?.as_deref() != Some(&self.credentials)
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    fn require_current_mapping(&self) -> Result<()> {
        if selection_ids(&self.library, &self.session, &self.token, &self.selection)? != self.songs
        {
            return Err(Error::QueueChanged);
        }
        self.require()
    }
}

#[derive(Clone)]
struct Attempt {
    target: String,
    operation: String,
}
#[derive(Default)]
struct Destinations {
    ticket: u64,
    ids: Vec<String>,
}
pub(super) struct Visit {
    source: Arc<Source>,
    pages: BTreeMap<(&'static str, u32), Destinations>,
    ticket: u64,
    attempt: Option<Attempt>,
    busy: bool,
}
type Slot = Mutex<Option<Visit>>;

fn with_visit<T>(
    slot: &Slot,
    source: &Arc<Source>,
    f: impl FnOnce(&mut Visit) -> Result<T>,
) -> Result<T> {
    let mut guard = slot.lock().map_err(|_| Error::LockPoisoned)?;
    f(guard
        .as_mut()
        .filter(|visit| Arc::ptr_eq(&visit.source, source))
        .ok_or(Error::StaleResponse)?)
}

struct Lease<'a>(&'a Slot, Arc<Source>);
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let _ = with_visit(self.0, &self.1, |visit| {
            visit.busy = false;
            Ok(())
        });
    }
}

impl MusicClient {
    pub async fn local_collect_prepare(
        &self,
        epoch: &str,
        mut selection: LocalCollectSelection,
    ) -> Result<AccountData<LocalCollectPlan>> {
        let token = self.playlist_token(epoch)?;
        selection.query.validate()?;
        if selection.query.group != LocalGroup::List
            || selection.query.offset != 0
            || selection.query.revision.is_some()
            || !(1..=GuardedPlaylistAdd::MAX_SONGS).contains(&selection.tracks.len())
        {
            return Err(Error::InvalidInput("invalid local collection selection"));
        }
        track_reference(&selection.revision, &selection.revision)?;
        for row in &selection.tracks {
            track_reference(&row.id, &row.revision)?;
        }
        selection.query.revision = Some(std::mem::take(&mut selection.revision));
        let api = self.session.native_api(&token)?;
        let credentials = api
            .local_collection_scope_revision()?
            .ok_or(Error::State("guarded local collection is unavailable"))?;
        let ticket = self
            .local
            .collection_prepare
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |sequence| {
                sequence.checked_add(1)
            })
            .map_err(|_| Error::State("local picker visits exhausted"))?
            + 1;
        let session = self.session.clone();
        let library = self.local.context().await?.library.clone();
        let source = tokio::task::spawn_blocking(move || {
            let songs = selection_ids(&library, &session, &token, &selection)?;
            Ok::<_, Error>(Arc::new(Source {
                token,
                session,
                api,
                credentials,
                library,
                selection,
                songs,
                receipt: uuid::Uuid::new_v4().simple().to_string(),
                created: Instant::now(),
            }))
        })
        .await
        .map_err(|_| Error::State("local collection worker stopped"))??;
        source.require()?;
        let plan = LocalCollectPlan {
            receipt: source.receipt.clone(),
            count: source.songs.len(),
        };
        let mut slot = self
            .local
            .collection
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        if self.local.collection_prepare.load(Ordering::Relaxed) != ticket {
            return Err(Error::StaleResponse);
        }
        if let Some(visit) = slot.as_ref() {
            if visit.source.token.generation > source.token.generation {
                return Err(Error::StaleResponse);
            }
            if visit.busy && visit.source.token == source.token {
                return Err(Error::MutationBusy);
            }
        }
        *slot = Some(Visit {
            source,
            pages: BTreeMap::new(),
            ticket: 0,
            attempt: None,
            busy: false,
        });
        Ok(AccountData {
            epoch: epoch.into(),
            value: plan,
        })
    }

    fn local_collect_source(&self, epoch: &str, receipt: &str) -> Result<Arc<Source>> {
        let slot = &self.local.collection;
        let source = slot
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .filter(|visit| {
                visit.source.receipt == receipt
                    && visit.source.token.generation.to_string() == epoch
            })
            .map(|visit| visit.source.clone())
            .ok_or(Error::StaleResponse)?;
        source.require()?;
        Ok(source)
    }

    pub fn local_collect_cancel(&self, epoch: &str, receipt: &str) -> Result<()> {
        let mut slot = self
            .local
            .collection
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        if slot.as_ref().is_some_and(|visit| {
            visit.source.receipt == receipt && visit.source.token.generation.to_string() == epoch
        }) {
            *slot = None;
        }
        Ok(())
    }

    pub async fn local_collect_destinations(
        &self,
        epoch: &str,
        receipt: &str,
        sort: PlaylistCollectSort,
        page: u32,
    ) -> Result<AccountData<PlaylistCollectPage>> {
        if page > crate::client::content::PLAYLIST_COLLECT_LAST_PAGE {
            return Err(Error::InvalidInput("invalid playlist collection page"));
        }
        let source = self.local_collect_source(epoch, receipt)?;
        let slot = &self.local.collection;
        let key = (sort.wire(), page);
        let ticket = with_visit(slot, &source, |visit| {
            if visit.busy || visit.attempt.is_some() {
                return Err(Error::MutationBusy);
            }
            visit.ticket = visit
                .ticket
                .checked_add(1)
                .ok_or(Error::State("picker reads exhausted"))?;
            visit.pages.insert(
                key,
                Destinations {
                    ticket: visit.ticket,
                    ..Default::default()
                },
            );
            Ok(visit.ticket)
        })?;
        let response = self
            .playlist_collect_page_scope(epoch, sort, page, true)
            .await?;
        source.require()?;
        with_visit(slot, &source, |visit| {
            if visit.busy || visit.attempt.is_some() {
                return Err(Error::StaleResponse);
            }
            let page = visit
                .pages
                .get_mut(&key)
                .filter(|page| page.ticket == ticket)
                .ok_or(Error::StaleResponse)?;
            page.ids = response
                .value
                .playlists
                .iter()
                .map(|row| row.id.clone())
                .collect();
            Ok(())
        })?;
        Ok(response)
    }

    /// Only an explicit click of an issued ordinary target can start a write.
    /// Reconciliation is a distinct, read-only action on the same journal ID.
    pub async fn local_collect_existing(
        &self,
        epoch: &str,
        receipt: &str,
        target: &str,
        reconcile: bool,
    ) -> Result<AccountData<LocalCollectResult>> {
        let source = self.local_collect_source(epoch, receipt)?;
        let slot = &self.local.collection;
        let attempt = with_visit(slot, &source, |visit| {
            if visit.busy {
                return Err(Error::MutationBusy);
            }
            let attempt = if reconcile {
                visit
                    .attempt
                    .as_ref()
                    .filter(|attempt| attempt.target == target)
                    .cloned()
                    .ok_or(Error::StaleResponse)?
            } else {
                if visit.attempt.is_some()
                    || !visit
                        .pages
                        .values()
                        .any(|page| page.ids.iter().any(|id| id == target))
                {
                    return Err(Error::StaleResponse);
                }
                let attempt = Attempt {
                    target: target.into(),
                    operation: uuid::Uuid::new_v4().simple().to_string(),
                };
                visit.attempt = Some(attempt.clone());
                attempt
            };
            visit.busy = true;
            Ok(attempt)
        })?;
        let _lease = Lease(slot, source.clone());
        let still_open = || {
            source.require()?;
            with_visit(slot, &source, |_| Ok(()))
        };
        let allowed_to_send = || {
            source.require_current_mapping()?;
            with_visit(slot, &source, |_| Ok(()))
        };
        let service = PlaylistService::new(self.session.clone())?;
        let record = if reconcile {
            service
                .reconcile_local_collect(
                    &source.token,
                    &attempt.operation,
                    target,
                    &source.songs,
                    &still_open,
                )
                .await?
        } else {
            match service
                .guarded_collect_local(
                    &source.token,
                    target,
                    &source.songs,
                    &attempt.operation,
                    &allowed_to_send,
                )
                .await
            {
                Ok(record) => record,
                Err(error) => match service.operation_for(&source.token, &attempt.operation)? {
                    Some(record) => record,
                    None => {
                        with_visit(slot, &source, |visit| {
                            visit.attempt = None;
                            Ok(())
                        })?;
                        // This journal is persisted before any dispatch. If no
                        // record was admitted, distinguish a failed preflight
                        // from a lost reply to an already-dispatched action.
                        let definitive = error.is_service_code(301)
                            || matches!(
                                error.root(),
                                Error::QueueChanged
                                    | Error::InvalidInput(_)
                                    | Error::StaleResponse
                                    | Error::CredentialChanged
                                    | Error::MutationBusy
                                    | Error::MutationUncertain
                            );
                        return Err(if definitive {
                            error
                        } else {
                            Error::RequestNotSent(Box::new(error))
                        });
                    }
                },
            }
        };
        still_open()?;
        if record.operation_id != attempt.operation
            || record.system_liked_group
            || !record.local_batch_favorite
            || !matches!(&record.command, PlaylistCommand::Tracks {id, change: TrackChange::Add, ids, ..}
                if id == target && ids == &source.songs)
        {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(&source.token, |_| Ok(()))?;
        let value = LocalCollectResult {
            phase: record.phase,
            duplicate: record.phase == PlaylistPhase::Rejected
                && record.codes.get("main") == Some(&502),
        };
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
