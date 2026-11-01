mod commands;
mod dto;
mod files;
mod selection;
mod view;
use super::{ClientError, MusicClient};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    cloud::{library::CloudLibrary, upload::*},
};
pub use dto::*;
use std::sync::{Arc, Mutex};
use tokio::sync::{Mutex as AsyncMutex, Notify};

pub(super) struct CloudHost {
    inner: Arc<Host>,
}
struct Host {
    session: Arc<AccountSession>,
    adapters: Mutex<Option<UploadAdapters>>,
    runtime: AsyncMutex<Option<Runtime>>,
}
struct Runtime {
    context: Arc<Context>,
    worker: tokio::task::JoinHandle<()>,
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.worker.abort();
    }
}
struct Context {
    session: Arc<AccountSession>,
    token: SessionToken,
    uploads: CloudUploads,
    scheduler: UploadScheduler,
    library: CloudLibrary,
    notify: Notify,
    worker_error: Mutex<Option<&'static str>>,
}
impl Context {
    fn guard(&self) -> Result<()> {
        self.session.with_token(&self.token, |_| Ok(()))
    }
}
impl CloudHost {
    pub(super) fn new(session: Arc<AccountSession>) -> Self {
        Self {
            inner: Arc::new(Host {
                session,
                adapters: Mutex::new(None),
                runtime: AsyncMutex::new(None),
            }),
        }
    }
}
impl Host {
    fn token(&self, epoch: &str) -> Result<SessionToken> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(token)
    }
    async fn context(&self, epoch: &str) -> Result<Arc<Context>> {
        let token = self.token(epoch)?;
        let mut runtime = self.runtime.lock().await;
        if let Some(runtime) = runtime.as_ref()
            && runtime.context.token == token
        {
            runtime.context.guard()?;
            return Ok(runtime.context.clone());
        }
        if let Some(mut old) = runtime.take() {
            old.worker.abort();
            let _ = (&mut old.worker).await;
        }
        let adapters = self
            .adapters
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .clone()
            .map(Ok)
            .unwrap_or_else(UploadAdapters::native)?;
        let uploads = CloudUploads::with_adapters(self.session.clone(), adapters)?;
        let scheduler = UploadScheduler::new(uploads.clone());
        scheduler.recover().await?;
        let context = Arc::new(Context {
            session: self.session.clone(),
            token,
            library: CloudLibrary::new(self.session.clone())?,
            uploads,
            scheduler,
            notify: Notify::new(),
            worker_error: Mutex::new(None),
        });
        context.guard()?;
        let captured = context.clone();
        let worker = tokio::spawn(async move {
            loop {
                if captured.guard().is_err() {
                    break;
                }
                match captured.scheduler.step().await {
                    Ok(Some(task)) => {
                        if task.phase == CloudPhase::Complete
                            && let Err(error) = captured.library.refresh().await
                            && !matches!(error, Error::MutationBusy | Error::StaleResponse)
                            && let Ok(mut value) = captured.worker_error.lock()
                        {
                            *value = Some("歌曲已上传，云盘列表暂未刷新，请重新加载");
                        }
                        continue;
                    }
                    Ok(None) => {}
                    Err(Error::StaleResponse) => break,
                    Err(error) => {
                        if let Ok(mut value) = captured.worker_error.lock() {
                            *value = Some(ClientError::from(error).message);
                        }
                        // A scheduler/storage failure needs an explicit command
                        // to wake it. Do not replay a failed workflow every poll.
                        captured.notify.notified().await;
                        continue;
                    }
                }
                tokio::select! {_=captured.notify.notified()=>{},_=tokio::time::sleep(std::time::Duration::from_millis(500))=>{}}
            }
        });
        *runtime = Some(Runtime {
            context: context.clone(),
            worker,
        });
        Ok(context)
    }
}
impl MusicClient {
    /// Complete current pending-task selection, independent of display page and
    /// uploaded-song filters. Captured IDs never grow during a later confirmation.
    pub async fn cloud_upload_selection(
        &self,
        epoch: &str,
        history: bool,
    ) -> Result<CloudPageView> {
        let context = self.cloud.inner.context(epoch).await?;
        tokio::task::spawn_blocking(move || {
            let result = view::project(
                &context,
                CloudQuery {
                    tab: if history {
                        CloudTab::History
                    } else {
                        CloudTab::Uploading
                    },
                    keyword: String::new(),
                    sort: CloudSort::Natural,
                    descending: false,
                    offset: 0,
                    limit: 50_000,
                },
            )?;
            if result.total > 50_000 {
                return Err(Error::InvalidInput(
                    "cloud task selection exceeds 50000 items",
                ));
            }
            Ok(result)
        })
        .await
        .map_err(|_| Error::State("cloud selection view worker stopped"))?
    }
    /// Read-only task removal receipts; does not repeat Cancel or delete objects.
    pub async fn cloud_upload_removals(
        &self,
        epoch: &str,
        ids: Vec<String>,
    ) -> Result<CloudUploadRemovalView> {
        if ids.is_empty()
            || ids.len() > 5000
            || ids.iter().collect::<std::collections::BTreeSet<_>>().len() != ids.len()
        {
            return Err(Error::InvalidInput("invalid cloud removal selection"));
        }
        let context = self.cloud.inner.context(epoch).await?;
        let epoch = epoch.to_owned();
        tokio::task::spawn_blocking(move || {
            let entries = context
                .scheduler
                .entries()?
                .into_iter()
                .map(|entry| (entry.id.clone(), entry))
                .collect::<std::collections::HashMap<_, _>>();
            let items = ids
                .into_iter()
                .map(|id| {
                    let task = context.uploads.get(&id)?;
                    let state = entries.get(&id);
                    Ok(CloudUploadRemovalItem {
                        removed: state.is_some_and(|s| s.hidden)
                            && matches!(task.phase, CloudPhase::Complete | CloudPhase::Cancelled),
                        requested: state.is_some_and(|s| s.removing)
                            || task.requested == Some(UploadControl::Cancel),
                        error: state
                            .and_then(|s| s.error.clone())
                            .or_else(|| task.error.map(|e| e.kind)),
                        id,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            context.guard()?;
            Ok(CloudUploadRemovalView { epoch, items })
        })
        .await
        .map_err(|_| Error::State("cloud removal view worker stopped"))?
    }
    pub(super) async fn cloud_resources(
        &self,
        epoch: &str,
        query: CloudQuery,
        revision: &str,
        start: Option<String>,
        quality: crate::membership::Quality,
    ) -> Result<(Vec<serde_json::Value>, usize)> {
        query.validate()?;
        if !matches!(query.tab, CloudTab::Uploaded) {
            return Err(Error::InvalidInput(
                "cloud playback requires uploaded songs",
            ));
        }
        let context = self.cloud.inner.context(epoch).await?;
        let snapshot = context.library.refresh().await?;
        if snapshot.revision != revision {
            return Err(Error::QueueChanged);
        }
        tokio::task::spawn_blocking(move || {
            context.guard()?;
            let tracks = view::catalog(Some(&snapshot), &query)?;
            if tracks.is_empty() || tracks.len() > 50000 {
                return Err(Error::InvalidInput("invalid cloud playback selection"));
            }
            let index = match start {
                Some(id) => tracks
                    .iter()
                    .position(|row| row.track.id == id)
                    .ok_or(Error::QueueChanged)?,
                None => 0,
            };
            let resources = tracks
                .into_iter()
                .map(|row| selection::resource(row, quality))
                .collect();
            context.guard()?;
            Ok((resources, index))
        })
        .await
        .map_err(|_| Error::State("cloud playback ordering worker stopped"))?
    }
    pub fn with_cloud_adapters(mut self, adapters: UploadAdapters) -> Self {
        Arc::get_mut(&mut self.cloud.inner)
            .expect("configure cloud before sharing client")
            .adapters = Mutex::new(Some(adapters));
        self
    }
    pub fn validate_cloud_epoch(&self, epoch: &str) -> Result<()> {
        self.cloud.inner.token(epoch).map(|_| ())
    }
    pub async fn select_cloud_sources(
        &self,
        epoch: &str,
        roots: Vec<std::path::PathBuf>,
    ) -> Result<Vec<String>> {
        let token = self.cloud.inner.token(epoch)?;
        let paths = tokio::task::spawn_blocking(move || select_audio_sources(&roots))
            .await
            .map_err(|_| Error::State("cloud selection worker stopped"))??;
        self.session.with_token(&token, |_| Ok(()))?;
        paths
            .into_iter()
            .map(|path| {
                path.into_os_string()
                    .into_string()
                    .map_err(|_| Error::InvalidInput("selected cloud path must be UTF8"))
            })
            .collect()
    }
    pub async fn cloud_page(
        &self,
        epoch: &str,
        query: CloudQuery,
        refresh: bool,
    ) -> Result<CloudPageView> {
        query.validate()?;
        let context = self.cloud.inner.context(epoch).await?;
        if refresh {
            context.library.refresh().await?;
            let mut error = context
                .worker_error
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if *error == Some("歌曲已上传，云盘列表暂未刷新，请重新加载") {
                *error = None;
            }
        }
        tokio::task::spawn_blocking(move || view::project(&context, query))
            .await
            .map_err(|_| Error::State("cloud view worker stopped"))?
    }
}
