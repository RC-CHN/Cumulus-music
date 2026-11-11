//! Shared receipt-bound playlist picker for downloads and recently played songs.
mod batch;
mod creation;
mod history;
mod liked;
mod lyrics;
use super::{Context, MusicClient, resource::catalogue_track_id};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    client::{
        content::PLAYLIST_COLLECT_LAST_PAGE,
        dto::{
            AccountData, HistoryCollectTarget, PlaylistCollectPage, PlaylistCollectSort,
            PlaylistMutationView,
        },
        playlists,
    },
    download::service::DownloadTask,
    history::{HistoryKind, HistoryService},
    playlist::{HistoryCollectDestination, PlaylistCommand, PlaylistService, TrackChange},
    protocol::{
        api::{Api, identifier},
        wire::serialization::js_truthy,
    },
};
pub use batch::DownloadBatchCollectPlan;
use creation::CreationAttempt;
pub use creation::{DownloadCollectCreation, DownloadCollectCreationCommand};
pub use history::HistoryCollectPlan;
pub(in crate::client) use history::OfflineSources as HistoryDownloadSources;
pub use liked::DownloadLikedDestination;
pub use lyrics::LyricGroupCollectPlan;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, atomic::Ordering},
    time::{Duration, Instant},
};

const VISIT_LIMIT: Duration = Duration::from_secs(300);

fn collection_resource(task: &DownloadTask) -> Option<(String, &Value)> {
    let id = catalogue_track_id(task)?;
    let metadata = task.song.as_ref()?;
    if identifier(&metadata["id"]).ok()?.as_str() != task.song_id
        || !metadata["voice_id"].is_null()
        || !metadata["voice"].is_null()
        || (!task.origin.is_cloud()
            && (!metadata["_cloud_public_id"].is_null()
                || !metadata["_cloud_catalog_id"].is_null()))
    {
        return None;
    }
    let p = &metadata["privilege"];
    if (!p.is_null() && !p.is_object())
        || (!p["id"].is_null() && identifier(&p["id"]).ok().as_deref() != Some(id.as_str()))
    {
        return None;
    }
    Some((id, p))
}

fn catalogued_song(task: &DownloadTask) -> Option<String> {
    let (id, p) = collection_resource(task)?;
    // M3085:780–783 retains falsey single-row subPriv (including numeric
    // zero), but a truthy privilege must coerce to a positive number.
    // Only known numeric wire representations are admissible here.
    for privilege in [&p["subp"], &p["subPriv"]] {
        if js_truthy(privilege)
            && !privilege
                .as_f64()
                .or_else(|| privilege.as_str()?.trim().parse().ok())
                .is_some_and(|n| n.is_finite() && n > 0.0)
        {
            return None;
        }
    }
    Some(id)
}

pub(super) fn eligible(task: &DownloadTask) -> bool {
    catalogued_song(task).is_some()
}

enum Selection {
    Single {
        task: String,
        songs: [String; 1],
        context: Box<Context>,
    },
    Batch {
        batch: batch::Batch,
        context: Box<Context>,
    },
    History {
        target: HistoryCollectTarget,
        songs: Vec<String>,
        offline: Option<Box<history::OfflineSources>>,
    },
    Lyrics {
        selection: Box<crate::client::search::lyric_collection::LyricCollectionSelection>,
        authority: Arc<crate::client::search::lyric_collection::LyricCollection>,
        login: Box<crate::protocol::api::PlaylistAddPermission<'static>>,
    },
}
struct Source {
    session: Arc<AccountSession>,
    token: SessionToken,
    api: Arc<dyn Api>,
    credentials: String,
    receipt: String,
    selection: Selection,
    created: Instant,
}

impl Source {
    fn songs(&self) -> &[String] {
        match &self.selection {
            Selection::Single { songs, .. } => songs,
            Selection::Batch { batch, .. } => &batch.songs,
            Selection::History { songs, .. } => songs,
            Selection::Lyrics { selection, .. } => &selection.songs,
        }
    }

    fn is_batch(&self) -> bool {
        matches!(self.selection, Selection::Batch { .. })
    }

    fn lyric_group(&self) -> bool {
        matches!(self.selection, Selection::Lyrics { .. })
    }

    fn ordered_group(&self) -> bool {
        matches!(
            self.selection,
            Selection::History { .. } | Selection::Lyrics { .. }
        )
    }

    fn regular_targets(&self) -> bool {
        self.is_batch() || self.lyric_group()
    }

    fn alternate_destination_songs(&self) -> Result<&[String]> {
        if self.is_batch() {
            return Err(Error::InvalidInput(
                "download batch requires an existing ordinary playlist",
            ));
        }
        Ok(self.songs())
    }

    fn require(&self) -> Result<()> {
        self.session.require_api(&self.token, &self.api)?;
        if self.created.elapsed() >= VISIT_LIMIT
            || self.api.download_collection_scope_revision()?.as_deref() != Some(&self.credentials)
        {
            return Err(Error::StaleResponse);
        }
        match &self.selection {
            Selection::Single {
                task,
                songs: [song],
                context,
            } => {
                if catalogued_song(&context.service.get_for(&self.token, task)?).as_ref()
                    != Some(song)
                {
                    return Err(Error::StaleResponse);
                }
            }
            Selection::Batch { batch, context } => batch.require(context)?,
            Selection::History {
                target, offline, ..
            } => {
                let current = HistoryService::new(self.session.clone())?
                    .snapshot_for(&self.token, HistoryKind::Song)?;
                if current.revision != target.revision() {
                    return Err(Error::QueueChanged);
                }
                if let Some(offline) = offline {
                    offline.require()?;
                }
            }
            Selection::Lyrics {
                selection,
                authority,
                login,
            } => {
                login()?;
                if self
                    .api
                    .lyric_group_collection_credential_revision()?
                    .as_deref()
                    != Some(self.credentials.as_str())
                    || !authority.current(selection)
                {
                    return Err(Error::StaleResponse);
                }
            }
        }
        self.session.with_token(&self.token, |_| Ok(()))
    }
}

struct Destinations {
    ticket: u64,
    ids: Vec<String>,
}

#[derive(Clone)]
struct Attempt {
    playlist: String,
    operation: String,
    system_liked: bool,
}

pub(super) struct Visit {
    source: Arc<Source>,
    destinations: BTreeMap<(&'static str, u32), Destinations>,
    ticket: u64,
    attempt: Option<Attempt>,
    creation: Option<CreationAttempt>,
    liked: Option<liked::Choice>,
    busy: bool,
}

fn with_visit<T>(
    slot: &Mutex<Option<Visit>>,
    source: &Arc<Source>,
    f: impl FnOnce(&mut Visit) -> Result<T>,
) -> Result<T> {
    let mut slot = slot.lock().map_err(|_| Error::LockPoisoned)?;
    f(slot
        .as_mut()
        .filter(|v| Arc::ptr_eq(&v.source, source))
        .ok_or(Error::StaleResponse)?)
}

struct Lease<'a> {
    slot: &'a Mutex<Option<Visit>>,
    source: Arc<Source>,
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        let _ = with_visit(self.slot, &self.source, |visit| {
            visit.busy = false;
            Ok(())
        });
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadCollectPlan {
    pub receipt: String,
    pub can_create: bool,
}

impl MusicClient {
    fn download_collection(&self, epoch: &str, receipt: &str) -> Result<Arc<Source>> {
        let source = self
            .downloads
            .inner
            .collect
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .filter(|v| {
                v.source.receipt == receipt && v.source.token.generation.to_string() == epoch
            })
            .map(|v| v.source.clone())
            .ok_or(Error::StaleResponse)?;
        source.require()?;
        Ok(source)
    }

    pub fn download_collect_prepare(
        &self,
        epoch: &str,
        task_id: &str,
    ) -> Result<AccountData<DownloadCollectPlan>> {
        let context = self.downloads.inner.context(epoch)?;
        let ticket = self.begin_download_collection()?;
        let song = catalogued_song(&context.service.get_for(&context.token, task_id)?).ok_or(
            Error::InvalidInput("download has no eligible catalogue collection target"),
        )?;
        let api = context.session.native_api(&context.token)?;
        let credentials = api
            .download_collection_scope_revision()?
            .ok_or(Error::State("guarded download collection is unavailable"))?;
        let receipt = uuid::Uuid::new_v4().simple().to_string();
        let source = Arc::new(Source {
            session: context.session.clone(),
            token: context.token.clone(),
            api,
            credentials,
            receipt: receipt.clone(),
            selection: Selection::Single {
                task: task_id.into(),
                songs: [song],
                context: Box::new(context),
            },
            created: Instant::now(),
        });
        source.require()?;
        let can_create = source
            .api
            .download_collection_create_scope_revision()
            .ok()
            .flatten()
            .as_deref()
            == Some(source.credentials.as_str());
        self.install_download_collection(source, ticket)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: DownloadCollectPlan {
                receipt,
                can_create,
            },
        })
    }

    fn begin_download_collection(&self) -> Result<u64> {
        self.downloads
            .inner
            .collect_prepare
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |previous| {
                previous.checked_add(1)
            })
            .map(|previous| previous + 1)
            .map_err(|_| Error::State("download picker visits exhausted"))
    }

    fn install_download_collection(&self, source: Arc<Source>, ticket: u64) -> Result<()> {
        source.require()?;
        let mut slot = self
            .downloads
            .inner
            .collect
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        if self.downloads.inner.collect_prepare.load(Ordering::Acquire) != ticket
            || slot
                .as_ref()
                .is_some_and(|visit| visit.source.token.generation > source.token.generation)
        {
            return Err(Error::StaleResponse);
        }
        if slot
            .as_ref()
            .is_some_and(|visit| visit.busy && visit.source.token == source.token)
        {
            return Err(Error::MutationBusy);
        }
        *slot = Some(Visit {
            source,
            destinations: BTreeMap::new(),
            ticket: 0,
            attempt: None,
            creation: None,
            liked: None,
            busy: false,
        });
        Ok(())
    }

    pub fn download_collect_cancel(&self, epoch: &str, receipt: &str) -> Result<()> {
        let mut slot = self
            .downloads
            .inner
            .collect
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        if slot.as_ref().is_some_and(|visit| {
            visit.source.receipt == receipt && visit.source.token.generation.to_string() == epoch
        }) {
            *slot = None;
        }
        Ok(())
    }

    pub async fn download_collect_destinations(
        &self,
        epoch: &str,
        receipt: &str,
        sort: PlaylistCollectSort,
        page: u32,
    ) -> Result<AccountData<PlaylistCollectPage>> {
        if page > PLAYLIST_COLLECT_LAST_PAGE {
            return Err(Error::InvalidInput("invalid playlist collection page"));
        }
        let source = self.download_collection(epoch, receipt)?;
        let slot = &self.downloads.inner.collect;
        let key = (sort.wire(), page);
        let ticket = with_visit(slot, &source, |visit| {
            if visit.busy || visit.attempt.is_some() || visit.creation.is_some() {
                return Err(Error::MutationBusy);
            }
            visit.ticket = visit
                .ticket
                .checked_add(1)
                .ok_or(Error::State("download picker reads exhausted"))?;
            visit.destinations.insert(
                key,
                Destinations {
                    ticket: visit.ticket,
                    ids: vec![],
                },
            );
            Ok(visit.ticket)
        })?;
        let reply = self
            .playlist_collect_page_scope(epoch, sort, page, source.regular_targets())
            .await?;
        source.require()?;
        with_visit(slot, &source, |visit| {
            let item = visit
                .destinations
                .get_mut(&key)
                .filter(|item| item.ticket == ticket)
                .ok_or(Error::StaleResponse)?;
            item.ids = reply.value.playlists.iter().map(|p| p.id.clone()).collect();
            Ok(())
        })?;
        Ok(reply)
    }

    pub async fn download_collect_existing(
        &self,
        epoch: &str,
        receipt: &str,
        playlist: &str,
    ) -> Result<AccountData<PlaylistMutationView>> {
        let source = self.download_collection(epoch, receipt)?;
        let slot = &self.downloads.inner.collect;
        let attempt = with_visit(slot, &source, |visit| {
            if visit.busy {
                return Err(Error::MutationBusy);
            }
            if visit.creation.is_some() {
                return Err(Error::StaleResponse);
            }
            if let Some(attempt) = &visit.attempt {
                if attempt.playlist != playlist || attempt.system_liked {
                    return Err(Error::StaleResponse);
                }
            } else {
                if !visit
                    .destinations
                    .values()
                    .any(|page| page.ids.iter().any(|id| id == playlist))
                {
                    return Err(Error::StaleResponse);
                }
                visit.attempt = Some(Attempt {
                    playlist: playlist.into(),
                    operation: uuid::Uuid::new_v4().simple().to_string(),
                    system_liked: false,
                });
            }
            visit.busy = true;
            Ok(visit.attempt.clone().expect("current attempt"))
        })?;
        let _lease = Lease {
            slot,
            source: source.clone(),
        };
        let check = || {
            source.require()?;
            // Check the live visit after the task/credential reads, so a
            // cancellation that wins while those reads are in progress also
            // wins the final synchronous dispatch permission.
            with_visit(slot, &source, |_| Ok(()))
        };
        let service = PlaylistService::new(source.session.clone())?;
        let record =
            if let Some(previous) = service.operation_for(&source.token, &attempt.operation)? {
                previous
            } else {
                let attempted = if source.ordered_group() {
                    let destination = if source.lyric_group() {
                        HistoryCollectDestination::ExistingRegular(playlist)
                    } else {
                        HistoryCollectDestination::Existing(playlist)
                    };
                    service
                        .guarded_collect_history(
                            &source.token,
                            destination,
                            source.songs(),
                            &attempt.operation,
                            &check,
                        )
                        .await
                } else {
                    service
                        .guarded_collect_download(
                            &source.token,
                            playlist,
                            source.songs(),
                            source.is_batch(),
                            &attempt.operation,
                            &check,
                        )
                        .await
                };
                match attempted {
                    Ok(record) => record,
                    Err(error) => {
                        // A response or readback error after admission must retain
                        // this exact operation, never infer it from another UI.
                        if let Some(record) =
                            service.operation_for(&source.token, &attempt.operation)?
                        {
                            record
                        } else {
                            with_visit(slot, &source, |visit| {
                                visit.attempt = None;
                                Ok(())
                            })?;
                            return Err(error);
                        }
                    }
                }
            };
        check()?;
        if record.operation_id != attempt.operation
            || record.system_liked_group
            || !matches!(&record.command, PlaylistCommand::Tracks { id, change: TrackChange::Add, ids, .. }
                if id == playlist && ids.as_slice() == source.songs())
        {
            return Err(Error::State(
                "download collection result belongs to another action",
            ));
        }
        let value = source
            .session
            .with_token(&source.token, |uid| playlists::view(record, uid))?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
