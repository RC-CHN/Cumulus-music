mod dto;
mod matching;
pub(super) mod view;
use super::MusicClient;
use crate::{
    Error, Result,
    library::{
        LocalLibrary,
        monitor::LibraryMonitor,
        probe::{AudioProbe, Ffprobe},
    },
    storage::StateStore,
};
pub use dto::*;
use std::{sync::Arc, time::Duration};
use tokio::sync::OnceCell;

pub(super) struct LocalHost {
    store: Arc<StateStore>,
    probe: Arc<dyn AudioProbe>,
    runtime: OnceCell<Arc<LocalRuntime>>,
    matching: tokio::sync::Mutex<Option<matching::Runtime>>,
}
pub(super) struct LocalRuntime {
    pub library: LocalLibrary,
    pub monitor: LibraryMonitor,
}
impl LocalHost {
    pub fn new(store: Arc<StateStore>) -> Self {
        Self {
            store,
            probe: Arc::new(Ffprobe::default()),
            runtime: OnceCell::new(),
            matching: tokio::sync::Mutex::new(None),
        }
    }
    pub async fn context(&self) -> Result<Arc<LocalRuntime>> {
        self.runtime
            .get_or_try_init(|| async {
                let store = self.store.clone();
                let probe = self.probe.clone();
                tokio::task::spawn_blocking(move || {
                    let library = LocalLibrary::with_probe(store, probe)?;
                    let monitor = LibraryMonitor::new(library.clone(), Duration::from_secs(30))?;
                    monitor.start()?;
                    Ok(Arc::new(LocalRuntime { library, monitor }))
                })
                .await
                .map_err(|_| Error::State("local library worker stopped"))?
            })
            .await
            .cloned()
    }
}
impl MusicClient {
    /// Native hosts can inject an explicit media probe before starting the library.
    pub fn with_local_probe(mut self, probe: Arc<dyn AudioProbe>) -> Self {
        self.local.probe = probe;
        self
    }
    /// Device-local state is intentionally independent of online credentials.
    pub async fn start_local_library(&self) -> Result<()> {
        self.local.context().await.map(|_| ())
    }
    pub async fn local_page(&self, query: LocalQuery) -> Result<LocalPageView> {
        query.validate()?;
        let context = self.local.context().await?;
        tokio::task::spawn_blocking(move || {
            // A scan publishes its catalog before marking the root Ready. Read
            // roots first so a completed status cannot describe an older page.
            let roots = context.monitor.roots()?.iter().map(view::root).collect();
            let snapshot = context.library.catalog(&query)?;
            let total = snapshot.items.len();
            let playable = snapshot.items.iter().filter(|i| i.available()).count();
            let tracks = snapshot
                .items
                .iter()
                .skip(query.offset)
                .take(query.limit)
                .map(view::row)
                .collect();
            Ok(LocalPageView {
                revision: snapshot.revision,
                tracks,
                groups: snapshot.groups,
                roots,
                total,
                playable,
                offset: query.offset,
                more: query.offset.saturating_add(query.limit) < total,
            })
        })
        .await
        .map_err(|_| Error::State("local library worker stopped"))?
    }
    /// Resolve a current indexed file for a native file-manager request. CUE
    /// resolves its physical audio source; NCM resolves the original, not cache.
    pub async fn local_track_directory(
        &self,
        id: String,
        revision: String,
    ) -> Result<std::path::PathBuf> {
        if [&id, &revision]
            .iter()
            .any(|value| value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(Error::InvalidInput("invalid local track reference"));
        }
        let context = self.local.context().await?;
        tokio::task::spawn_blocking(move || {
            let item = context.library.item(&id)?;
            if view::revision(&item.revision) != revision {
                return Err(Error::QueueChanged);
            }
            LocalLibrary::check_current(&item)?;
            if item.path.canonicalize()? != item.path {
                return Err(Error::QueueChanged);
            }
            let parent = item
                .path
                .parent()
                .ok_or(Error::State("local source has no parent directory"))?;
            Ok(parent.to_path_buf())
        })
        .await
        .map_err(|_| Error::State("local library worker stopped"))?
    }
    pub async fn local_command(&self, command: LocalCommand) -> Result<()> {
        let context = self.local.context().await?;
        tokio::task::spawn_blocking(move || {
            match command {
                LocalCommand::Watch { path } => {
                    context.monitor.watch(path)?;
                }
                LocalCommand::Disable { id } => {
                    context
                        .monitor
                        .unwatch(&id)?
                        .ok_or(Error::InvalidInput("unknown library root"))?;
                }
                LocalCommand::Enable { id } => {
                    let root = context
                        .monitor
                        .roots()?
                        .into_iter()
                        .find(|r| r.id == id)
                        .ok_or(Error::InvalidInput("unknown library root"))?;
                    context.monitor.watch(root.path)?;
                }
                LocalCommand::Rescan => context.monitor.rescan()?,
            }
            Ok(())
        })
        .await
        .map_err(|_| Error::State("local library worker stopped"))?
    }
}
