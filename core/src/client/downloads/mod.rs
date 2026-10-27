mod commands;
mod dto;
mod jobs;
mod view;
use super::MusicClient;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    download::{
        completed::CompletedDownloads,
        directory::DownloadDirectory,
        service::{DownloadScheduler, DownloadService},
        transfer::{MediaBackend, ReqwestMediaBackend},
    },
    storage::StateStore,
};
pub use dto::*;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

const PREFERENCES: &str = "download_preferences_v1";
pub(super) struct DownloadHost {
    inner: Arc<Host>,
}
struct Host {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    backend: Mutex<Option<Arc<dyn MediaBackend>>>,
    worker: Mutex<Option<Worker>>,
    jobs: Mutex<HashMap<String, Arc<LocalJob>>>,
}
struct Worker {
    token: SessionToken,
    notify: Arc<Notify>,
    scheduler: Arc<DownloadScheduler>,
    handle: tokio::task::JoinHandle<()>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.scheduler.stop();
        self.handle.abort();
    }
}
struct LocalJob {
    token: SessionToken,
    id: String,
    kind: &'static str,
    stop: Arc<AtomicBool>,
    cancel: AtomicBool,
    running: AtomicBool,
    error: Mutex<Option<&'static str>>,
}
impl Drop for Host {
    fn drop(&mut self) {
        if let Ok(jobs) = self.jobs.lock() {
            for job in jobs.values() {
                job.stop.store(true, Ordering::Release);
            }
        }
    }
}
#[derive(Clone)]
struct Context {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    token: SessionToken,
    owner: String,
    service: DownloadService,
    files: CompletedDownloads,
    directory: DownloadDirectory,
}
impl Context {
    fn guard(&self) -> Result<()> {
        self.session.with_token(&self.token, |_| Ok(()))
    }
}
impl DownloadHost {
    pub(super) fn new(session: Arc<AccountSession>, store: Arc<StateStore>) -> Self {
        Self {
            inner: Arc::new(Host {
                session,
                store,
                backend: Mutex::new(None),
                worker: Mutex::new(None),
                jobs: Mutex::new(HashMap::new()),
            }),
        }
    }
}
impl Host {
    fn context(&self, epoch: &str) -> Result<Context> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        let owner = self.session.with_token(&token, |uid| Ok(uid.to_owned()))?;
        let backend = {
            let mut backend = self.backend.lock().map_err(|_| Error::LockPoisoned)?;
            if backend.is_none() {
                *backend = Some(Arc::new(ReqwestMediaBackend::new(Duration::from_secs(30))?));
            }
            backend.as_ref().expect("initialized backend").clone()
        };
        let context = Context {
            session: self.session.clone(),
            store: self.store.clone(),
            token,
            owner,
            service: DownloadService::with_backend(
                self.session.clone(),
                backend,
                512 * 1024 * 1024,
            )?,
            files: CompletedDownloads::new(self.session.clone())?,
            directory: DownloadDirectory::new(self.session.clone())?,
        };
        context.guard()?;
        Ok(context)
    }
    fn kick(&self, context: &Context) -> Result<()> {
        context.guard()?;
        let mut worker = self.worker.lock().map_err(|_| Error::LockPoisoned)?;
        if let Some(current) = worker.as_ref()
            && current.token == context.token
            && !current.handle.is_finished()
        {
            current.notify.notify_one();
            return Ok(());
        }
        let scheduler = Arc::new(DownloadScheduler::new(context.service.clone(), 3)?);
        let notify = Arc::new(Notify::new());
        let wake = notify.clone();
        let jobs = scheduler.clone();
        let captured = context.clone();
        let handle = tokio::spawn(async move {
            loop {
                if captured.guard().is_err() {
                    break;
                }
                if captured.directory.active().ok().flatten().is_none() {
                    let _ = jobs.run_pending().await;
                }
                wake.notified().await;
            }
        });
        *worker = Some(Worker {
            token: context.token.clone(),
            notify,
            scheduler,
            handle,
        });
        Ok(())
    }
}
impl MusicClient {
    pub(super) async fn download_resources(
        &self,
        epoch: &str,
        ids: Vec<String>,
    ) -> Result<Vec<serde_json::Value>> {
        if ids.is_empty() || ids.len() > 5000 {
            return Err(Error::InvalidInput("invalid local download selection"));
        }
        let context = self.downloads.inner.context(epoch)?;
        tokio::task::spawn_blocking(move || {
            context.guard()?;
            let mut resources=Vec::with_capacity(ids.len());
            for id in ids {
                let task=context.service.get(&id)?;
                if task.origin.is_mv(){return Err(Error::State("MV downloads require the video player"));}
                if task.status!=crate::download::service::DownloadStatus::Complete{return Err(Error::State("download is not complete"));}
                let path=task.target.ok_or(Error::State("download target missing"))?;
                let track=super::mapping::track(task.song.as_ref().ok_or(Error::State("download metadata missing"))?)?;
                resources.push(serde_json::json!({"id":task.song_id,"cloud_song_id":if task.origin.is_cloud(){Some(task.song_id.clone())}else{None},"download_task_id":task.id,"path":path,"dt":track.duration_ms,
                    "level":task.options.quality.as_str(),"song":{"id":track.id,"name":track.name,"ar":track.artists.iter().map(|name|serde_json::json!({"name":name})).collect::<Vec<_>>(),
                    "al":{"name":track.album,"picUrl":track.cover_url},"voice_id":track.voice_id,"dt":track.duration_ms,"fee":track.fee}}));
            }
            context.guard()?;Ok(resources)
        }).await.map_err(|_|Error::State("local selection worker stopped"))?
    }
    /// Native injection for fixture media backends; set before starting downloads.
    pub fn with_download_backend(mut self, backend: Arc<dyn MediaBackend>) -> Self {
        Arc::get_mut(&mut self.downloads.inner)
            .expect("configure downloads before sharing client")
            .backend = Mutex::new(Some(backend));
        self
    }
    pub async fn downloads(&self, epoch: &str, query: DownloadQuery) -> Result<DownloadsView> {
        super::mapping::page(query.offset, query.limit)?;
        if query.keyword.len() > 1000 {
            return Err(Error::InvalidInput("download keyword too long"));
        }
        let context = self.downloads.inner.context(epoch)?;
        let host = self.downloads.inner.clone();
        // Restore only persisted runnable tasks. Paused/error and migrations need an explicit action.
        host.kick(&context)?;
        tokio::task::spawn_blocking(move || host.view(&context, query))
            .await
            .map_err(|_| Error::State("download view worker stopped"))?
    }
    pub fn validate_download_epoch(&self, epoch: &str) -> Result<()> {
        self.downloads.inner.context(epoch).map(|_| ())
    }
    pub async fn download_directory_path(
        &self,
        epoch: &str,
        task_id: Option<String>,
    ) -> Result<std::path::PathBuf> {
        let context = self.downloads.inner.context(epoch)?;
        tokio::task::spawn_blocking(move || {
            context.guard()?;
            let path = if let Some(id) = task_id {
                let task = context.service.get(&id)?;
                task.target
                    .ok_or(Error::State("download target not resolved"))?
                    .parent()
                    .ok_or(Error::State("download lacks parent"))?
                    .to_path_buf()
            } else {
                context
                    .directory
                    .settings()?
                    .ok_or(Error::State("download directory not configured"))?
                    .root
            };
            let path = path.canonicalize()?;
            if !path.is_dir() {
                return Err(Error::State("download directory missing"));
            }
            context.guard()?;
            Ok(path)
        })
        .await
        .map_err(|_| Error::State("directory view worker stopped"))?
    }
}
