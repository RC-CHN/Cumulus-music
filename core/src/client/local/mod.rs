mod collect;
mod directories;
mod dto;
mod matching;
pub(super) mod view;
use super::MusicClient;
pub use crate::library::monitor::{DirectoryChoice, DirectorySelection};
use crate::{
    Error, Result,
    library::{
        LocalLibrary,
        monitor::LibraryMonitor,
        probe::{AudioProbe, Ffprobe},
    },
    storage::StateStore,
};
pub use collect::{LocalCollectPlan, LocalCollectResult, LocalCollectSelection};
pub use dto::*;
use std::{sync::Arc, time::Duration};
use tokio::sync::OnceCell;

pub(super) struct LocalHost {
    store: Arc<StateStore>,
    probe: Arc<dyn AudioProbe>,
    runtime: OnceCell<Arc<LocalRuntime>>,
    matching: tokio::sync::Mutex<Option<matching::Runtime>>,
    collection: std::sync::Mutex<Option<collect::Visit>>,
    collection_prepare: std::sync::atomic::AtomicU64,
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
            collection: std::sync::Mutex::new(None),
            collection_prepare: std::sync::atomic::AtomicU64::new(0),
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
    pub async fn local_track_file(
        &self,
        id: String,
        revision: String,
    ) -> Result<std::path::PathBuf> {
        track_reference(&id, &revision)?;
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
            Ok(item.path)
        })
        .await
        .map_err(|_| Error::State("local library worker stopped"))?
    }
    /// Reveal the exact folder group selected in the current local index.
    /// The UI's group key is a selector, never a path passed to the shell.
    pub async fn local_folder_directory(
        &self,
        mut query: LocalQuery,
        revision: String,
    ) -> Result<std::path::PathBuf> {
        if query.group != LocalGroup::Folder
            || query.group_key.as_deref().is_none_or(str::is_empty)
            || query.offset != 0
            || query.revision.is_some()
            || revision.len() != 64
            || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(Error::InvalidInput("invalid local folder reference"));
        }
        query.revision = Some(revision);
        query.validate()?;
        let context = self.local.context().await?;
        tokio::task::spawn_blocking(move || {
            let snapshot = context.library.catalog(&query)?;
            let mut members = snapshot.items.iter();
            let parent = members
                .next()
                .and_then(|item| item.path.parent())
                .ok_or(Error::QueueChanged)?;
            // Folder keys are projected as text. Refuse a lossy-key collision
            // rather than opening whichever native parent happened to sort first.
            if members.any(|item| item.path.parent() != Some(parent)) {
                return Err(Error::QueueChanged);
            }
            if !parent.is_absolute() || !parent.is_dir() || parent.canonicalize()? != parent {
                return Err(Error::QueueChanged);
            }
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
                        .find(|r| r.id == id && r.checked)
                        .ok_or(Error::InvalidInput("unknown library root"))?;
                    context.monitor.watch(root.path)?;
                }
                LocalCommand::Rescan => {
                    context.library.restore_hidden()?;
                    context.monitor.rescan()?;
                }
                LocalCommand::HideTrack { id, revision } => {
                    track_reference(&id, &revision)?;
                    context.library.hide_revision(&id, &revision)?;
                }
                LocalCommand::HideGroup { query, revision } => {
                    query.validate()?;
                    if query.offset != 0
                        || query.revision.is_some()
                        || revision.len() != 64
                        || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
                    {
                        return Err(Error::InvalidInput("invalid local group reference"));
                    }
                    context.library.hide_group(&query, &revision)?;
                }
                LocalCommand::HideTracks {
                    query,
                    revision,
                    tracks,
                } => {
                    query.validate()?;
                    if query.group != LocalGroup::List
                        || query.offset != 0
                        || query.revision.is_some()
                        || revision.len() != 64
                        || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
                        || !(1..=50_000).contains(&tracks.len())
                    {
                        return Err(Error::InvalidInput("invalid local batch reference"));
                    }
                    let tracks = tracks
                        .iter()
                        .map(|track| {
                            track_reference(&track.id, &track.revision)?;
                            Ok((track.id.as_str(), track.revision.as_str()))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    context.library.hide_selection(&query, &revision, &tracks)?;
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| Error::State("local library worker stopped"))?
    }
}

pub(super) fn track_reference(id: &str, revision: &str) -> Result<()> {
    if [id, revision]
        .into_iter()
        .any(|value| value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(Error::InvalidInput("invalid local track reference"));
    }
    Ok(())
}
