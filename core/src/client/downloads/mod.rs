mod collect;
mod commands;
mod comments;
mod dto;
mod jobs;
mod quality;
mod resource;
mod song_status;
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
pub use collect::DownloadCollectPlan;
pub use dto::*;
pub use quality::DownloadQualityPreferences;
pub use song_status::SongDownloadState;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

pub(in crate::client) struct GuardedSongBatch {
    pub receipt: String,
    pub anchor: String,
    pub songs: Arc<[String]>,
    pub options: crate::download::service::DownloadOptions,
}

const PREFERENCES: &str = "download_preferences_v1";

// A download's typed origin owns its namespace. Raw metadata remains intact in
// the task record, but cannot turn a Song/Cloud download into an episode while
// projecting the list or constructing a new playback resource.
fn native_metadata(task: &crate::download::service::DownloadTask) -> Option<serde_json::Value> {
    let mut metadata = task.song.clone()?;
    if let Some(fields) = metadata.as_object_mut() {
        match &task.origin {
            crate::download::service::DownloadOrigin::Voice { voice_id } => {
                fields.insert("voice_id".into(), serde_json::json!(voice_id));
            }
            _ => {
                fields.remove("voice_id");
                fields.remove("voice");
            }
        }
    }
    Some(metadata)
}

fn voice_podcast(
    task: &crate::download::service::DownloadTask,
) -> Option<DownloadVoicePodcastView> {
    use crate::{
        download::service::{DownloadOrigin, DownloadStatus},
        protocol::api::identifier,
    };
    let DownloadOrigin::Voice { voice_id } = &task.origin else {
        return None;
    };
    if task.status != DownloadStatus::Complete {
        return None;
    }
    let metadata = task.song.as_ref()?;
    let voice = &metadata["voice"];
    // Bind the source to the downloaded episode and its own media ID;
    // ordinary song metadata and display artist/album IDs are unrelated.
    if identifier(&metadata["id"]).ok()? != task.song_id
        || identifier(&voice["id"]).ok()? != *voice_id
        || identifier(&voice["mainTrackId"]).ok()? != task.song_id
    {
        return None;
    }
    let audio = &voice["audio"];
    let name = audio["name"]
        .as_str()
        .filter(|value| !value.trim().is_empty() && value.len() <= 4096)?;
    Some(DownloadVoicePodcastView {
        name: name.into(),
        id: identifier(&audio["id"]).ok(),
    })
}

pub(super) struct DownloadHost {
    inner: Arc<Host>,
}
struct Host {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    backend: Mutex<Option<Arc<dyn MediaBackend>>>,
    worker: Mutex<Option<Worker>>,
    jobs: Mutex<HashMap<String, Arc<LocalJob>>>,
    collect: Mutex<Option<collect::Visit>>,
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
                collect: Mutex::new(None),
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
    pub(super) fn retire_account_downloads(&self, previous: &SessionToken) -> Result<()> {
        let host = &self.downloads.inner;
        {
            let worker = host.worker.lock().map_err(|_| Error::LockPoisoned)?;
            if let Some(worker) = worker.as_ref().filter(|worker| &worker.token == previous) {
                // Let the previous scheduler observe its stop/epoch fence.
                // Do not abort a task in the middle of file recovery.
                worker.scheduler.stop();
                worker.notify.notify_one();
            }
        }
        let jobs = host.jobs.lock().map_err(|_| Error::LockPoisoned)?;
        for job in jobs.values().filter(|job| &job.token == previous) {
            job.stop.store(true, Ordering::Release);
        }
        Ok(())
    }
    pub(in crate::client) async fn cloud_removal_downloads(
        &self,
        epoch: &str,
    ) -> Result<Vec<crate::download::service::DownloadTask>> {
        let context = self.downloads.inner.context(epoch)?;
        tokio::task::spawn_blocking(move || {
            context.guard()?;
            let mut tasks = context
                .service
                .list()?
                .into_iter()
                .filter(|task| task.origin.is_cloud())
                .collect::<Vec<_>>();
            if tasks.len() > 50_000 {
                return Err(Error::State("cloud download associations exceed limit"));
            }
            tasks.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then(b.id.cmp(&a.id)));
            context.guard()?;
            Ok(tasks)
        })
        .await
        .map_err(|_| Error::State("cloud removal association worker stopped"))?
    }
    pub(in crate::client) async fn cloud_file_downloads(
        &self,
        epoch: &str,
        cloud_id: String,
        public_id: Option<String>,
    ) -> Result<Vec<crate::download::service::DownloadTask>> {
        let context = self.downloads.inner.context(epoch)?;
        tokio::task::spawn_blocking(move || {
            use crate::download::service::{DownloadOrigin, DownloadStatus};
            context.guard()?;
            let mut tasks = context
                .service
                .list()?
                .into_iter()
                .filter(|task| {
                    task.status == DownloadStatus::Complete
                        && task.file_operation.is_none()
                        && task.target.is_some()
                        && task.output_integrity.is_some()
                        && match &task.origin {
                            DownloadOrigin::Cloud { .. } => task.song_id == cloud_id,
                            DownloadOrigin::Song => public_id.as_ref() == Some(&task.song_id),
                            _ => false,
                        }
                })
                .collect::<Vec<_>>();
            tasks.sort_by_key(|t| !matches!(t.origin, DownloadOrigin::Cloud { .. }));
            context.guard()?;
            Ok(tasks)
        })
        .await
        .map_err(|_| Error::State("cloud file association worker stopped"))?
    }
    /// Official trackMap is keyed by catalog song ID, never by title, voice,
    /// cloud ID or a scanned file. For this header use the newest completed
    /// publication for that exact song, and let the source resolver verify it.
    pub(super) async fn follow_latest_download(
        &self,
        epoch: &str,
        song: &str,
    ) -> Result<Option<(String, std::path::PathBuf)>> {
        use crate::download::service::DownloadOrigin;
        let context = self.downloads.inner.context(epoch)?;
        let song = song.to_owned();
        tokio::task::spawn_blocking(move || {
            context.guard()?;
            let candidate = context
                .service
                .list()?
                .into_iter()
                .filter(|task| {
                    task.origin == DownloadOrigin::Song
                        && task.song_id == song
                        && task.output_integrity.is_some()
                        && task.playback_target().is_ok()
                })
                .max_by_key(|task| (task.completed_ms, task.created_ms, task.id.clone()))
                .map(|task| Ok::<_, Error>((task.id.clone(), task.playback_target()?.to_owned())))
                .transpose()?;
            context.guard()?;
            Ok(candidate)
        })
        .await
        .map_err(|_| Error::State("download identity worker stopped"))?
    }

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
                let path=task.playback_target()?;
                let metadata = native_metadata(&task).ok_or(Error::State("download metadata missing"))?;
                let track=super::mapping::track(&metadata)?;
                // Native origin authority is separate from arbitrary song JSON.
                // Never infer an episode from a Song/Cloud task's raw metadata.
                let download_voice_id = match &task.origin {
                    crate::download::service::DownloadOrigin::Voice { voice_id } => Some(voice_id),
                    _ => None,
                };
                let mut song = super::mapping::projected_song_metadata(&track);
                song["voice_id"] = serde_json::json!(track.voice_id);
                resources.push(serde_json::json!({"id":task.song_id,"cloud_song_id":if task.origin.is_cloud(){Some(task.song_id.clone())}else{None},"download_task_id":task.id,"path":path,"dt":track.duration_ms,
                    "download_voice_id":download_voice_id,
                    "voice":metadata["voice"],
                    "level":task.options.quality.as_str(),"song":song}));
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
        query.validate()?;
        let context = self.downloads.inner.context(epoch)?;
        let host = self.downloads.inner.clone();
        // Restore only persisted runnable tasks. Paused/error and migrations need an explicit action.
        host.kick(&context)?;
        tokio::task::spawn_blocking(move || host.view(&context, query))
            .await
            .map_err(|_| Error::State("download view worker stopped"))?
    }
    pub(super) async fn download_selection(
        &self,
        epoch: &str,
        mut query: DownloadQuery,
        start_id: Option<String>,
    ) -> Result<(Vec<serde_json::Value>, usize)> {
        query.validate()?;
        if query.tab != DownloadTab::Completed || query.category == DownloadCategory::Mv {
            return Err(Error::InvalidInput(
                "download selection requires completed audio",
            ));
        }
        // The native audio queue has a 5000-item bound. Read one extra item to
        // reject oversized selections explicitly instead of silently truncating.
        query.offset = 0;
        query.limit = 5001;
        let context = self.downloads.inner.context(epoch)?;
        let host = self.downloads.inner.clone();
        let view = tokio::task::spawn_blocking(move || host.view(&context, query))
            .await
            .map_err(|_| Error::State("download selection worker stopped"))??;
        if view.total > 5000 {
            return Err(Error::InvalidInput("download selection exceeds 5000 items"));
        }
        let ids: Vec<_> = view
            .tasks
            .into_iter()
            .filter(|t| t.source != "mv" && t.file_operation.is_none() && t.path.is_some())
            .map(|t| t.id)
            .collect();
        let index = match start_id {
            Some(id) => ids
                .iter()
                .position(|item| item == &id)
                .ok_or(Error::StaleResponse)?,
            None => 0,
        };
        let resources = self.download_resources(epoch, ids).await?;
        Ok((resources, index))
    }
    pub fn validate_download_epoch(&self, epoch: &str) -> Result<()> {
        self.downloads.inner.context(epoch).map(|_| ())
    }
    pub(super) async fn configured_download_directory_available(
        &self,
        epoch: &str,
    ) -> Result<bool> {
        let context = self.downloads.inner.context(epoch)?;
        tokio::task::spawn_blocking(move || {
            context.guard()?;
            let available = match context.directory.settings()? {
                None => false,
                Some(settings) => match settings.root.canonicalize() {
                    Ok(path) => path == settings.root && path.is_dir(),
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                        ) =>
                    {
                        false
                    }
                    Err(error) => return Err(error.into()),
                },
            };
            context.guard()?;
            Ok(available)
        })
        .await
        .map_err(|_| Error::State("directory view worker stopped"))?
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
