//! Receipt-bound existing-playlist collection from completed downloads.
use super::{Context, MusicClient, resource::catalogue_track_id};
use crate::{
    Error, Result,
    client::{
        content::PLAYLIST_COLLECT_LAST_PAGE,
        dto::{AccountData, PlaylistCollectPage, PlaylistCollectSort, PlaylistMutationView},
        playlists,
    },
    download::service::DownloadTask,
    playlist::{PlaylistCommand, PlaylistService, TrackChange},
    protocol::{
        api::{Api, identifier},
        wire::serialization::js_truthy,
    },
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const VISIT_LIMIT: Duration = Duration::from_secs(300);

fn catalogued_song(task: &DownloadTask) -> Option<String> {
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

struct Source {
    context: Context,
    api: Arc<dyn Api>,
    credentials: String,
    receipt: String,
    task: String,
    song: String,
    created: Instant,
}

impl Source {
    fn require(&self) -> Result<()> {
        let ctx = &self.context;
        ctx.session.require_api(&ctx.token, &self.api)?;
        if self.created.elapsed() >= VISIT_LIMIT
            || self.api.download_collection_scope_revision()?.as_deref() != Some(&self.credentials)
            || catalogued_song(&ctx.service.get_for(&ctx.token, &self.task)?).as_deref()
                != Some(&self.song)
        {
            return Err(Error::StaleResponse);
        }
        ctx.guard()
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
}

pub(super) struct Visit {
    source: Arc<Source>,
    destinations: BTreeMap<(&'static str, u32), Destinations>,
    ticket: u64,
    attempt: Option<Attempt>,
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
                v.source.receipt == receipt
                    && v.source.context.token.generation.to_string() == epoch
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
        let song = catalogued_song(&context.service.get_for(&context.token, task_id)?).ok_or(
            Error::InvalidInput("download has no eligible catalogue collection target"),
        )?;
        let api = context.session.native_api(&context.token)?;
        let credentials = api
            .download_collection_scope_revision()?
            .ok_or(Error::State("guarded download collection is unavailable"))?;
        let receipt = uuid::Uuid::new_v4().simple().to_string();
        let source = Arc::new(Source {
            context,
            api,
            credentials,
            receipt: receipt.clone(),
            task: task_id.into(),
            song,
            created: Instant::now(),
        });
        source.require()?;
        let mut slot = self
            .downloads
            .inner
            .collect
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        if slot.as_ref().is_some_and(|visit| {
            visit.source.context.token.generation > source.context.token.generation
        }) {
            return Err(Error::StaleResponse);
        }
        if slot
            .as_ref()
            .is_some_and(|visit| visit.busy && visit.source.context.token == source.context.token)
        {
            return Err(Error::MutationBusy);
        }
        *slot = Some(Visit {
            source,
            destinations: BTreeMap::new(),
            ticket: 0,
            attempt: None,
            busy: false,
        });
        Ok(AccountData {
            epoch: epoch.into(),
            value: DownloadCollectPlan { receipt },
        })
    }

    pub fn download_collect_cancel(&self, epoch: &str, receipt: &str) -> Result<()> {
        let mut slot = self
            .downloads
            .inner
            .collect
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        if slot.as_ref().is_some_and(|visit| {
            visit.source.receipt == receipt
                && visit.source.context.token.generation.to_string() == epoch
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
            if visit.busy || visit.attempt.is_some() {
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
        let reply = self.playlist_collect_page(epoch, sort, page).await?;
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
        let ctx = &source.context;
        let slot = &self.downloads.inner.collect;
        let attempt = with_visit(slot, &source, |visit| {
            if visit.busy {
                return Err(Error::MutationBusy);
            }
            if let Some(attempt) = &visit.attempt {
                if attempt.playlist != playlist {
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
            with_visit(slot, &source, |_| Ok(()))?;
            source.require()
        };
        let service = PlaylistService::new(ctx.session.clone())?;
        let record = if let Some(previous) =
            service.operation_for(&ctx.token, &attempt.operation)?
        {
            previous
        } else {
            let attempted = service
                .guarded_collect_download(
                    &ctx.token,
                    playlist,
                    &source.song,
                    &attempt.operation,
                    &check,
                )
                .await;
            match attempted {
                Ok(record) => record,
                Err(error) => {
                    // A response or readback error after admission must retain
                    // this exact operation, never infer it from another UI.
                    if let Some(record) = service.operation_for(&ctx.token, &attempt.operation)? {
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
                if id == playlist && ids.len() == 1 && ids[0] == source.song)
        {
            return Err(Error::State(
                "download collection result belongs to another action",
            ));
        }
        let value = ctx
            .session
            .with_token(&ctx.token, |uid| playlists::view(record, uid))?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
