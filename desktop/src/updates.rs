use cumulus_core::updates::{
    GitHubReleases, LatestReleaseSource, PackageFailure, PackageVerification, PublishedPackages,
    ReleasePage, UpdateCheck, UpdateFailure, check_latest,
};
use std::{path::PathBuf, sync::Arc};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

#[derive(Default)]
struct Snapshot {
    page: Option<ReleasePage>,
    packages: Option<Arc<PublishedPackages>>,
}

#[derive(Default)]
pub(crate) struct UpdateGate(tokio::sync::Mutex<Snapshot>);

#[derive(Clone)]
pub struct UpdateAdapters {
    source: Arc<dyn LatestReleaseSource>,
    launch: Arc<dyn Fn(ReleasePage) -> Result<(), UpdateFailure> + Send + Sync>,
    package_picker: Option<Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>>,
}

impl UpdateAdapters {
    /// Native dependency injection for closed test hosts. The renderer cannot
    /// provide a source, launcher, version, repository or address.
    pub fn with_adapters(
        source: impl LatestReleaseSource + 'static,
        launch: impl Fn(ReleasePage) -> Result<(), UpdateFailure> + Send + Sync + 'static,
    ) -> Self {
        Self {
            source: Arc::new(source),
            launch: Arc::new(launch),
            package_picker: None,
        }
    }

    /// Native-only picker override for a closed integration host.
    pub fn with_package_picker(
        mut self,
        picker: impl Fn() -> Option<PathBuf> + Send + Sync + 'static,
    ) -> Self {
        self.package_picker = Some(Arc::new(picker));
        self
    }

    /// Closed hosts can replace only the HTTP source and exercise the same
    /// platform browser implementation as production.
    pub fn with_source(source: impl LatestReleaseSource + 'static) -> Self {
        Self::with_adapters(source, |page| {
            crate::browser::launch(&page.url()).map_err(|_| UpdateFailure::Browser)
        })
    }
}
impl Default for UpdateAdapters {
    fn default() -> Self {
        Self::with_source(GitHubReleases)
    }
}

fn adapters<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> UpdateAdapters {
    window
        .try_state::<UpdateAdapters>()
        .map(|state| state.inner().clone())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn check_for_updates<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    gate: tauri::State<'_, UpdateGate>,
) -> Result<UpdateCheck, UpdateFailure> {
    if window.label() != "main" {
        return Err(UpdateFailure::Unavailable);
    }
    let mut candidate = gate.0.try_lock().map_err(|_| UpdateFailure::Busy)?;
    *candidate = Snapshot::default();
    let result = check_latest(env!("CARGO_PKG_VERSION"), adapters(&window).source.as_ref()).await?;
    candidate.page = result.release;
    candidate.packages = result.packages.map(Arc::new);
    Ok(result.view)
}

#[tauri::command]
pub async fn open_update_release_page<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    gate: tauri::State<'_, UpdateGate>,
) -> Result<(), UpdateFailure> {
    if window.label() != "main" {
        return Err(UpdateFailure::Unavailable);
    }
    let mut candidate = gate.0.try_lock().map_err(|_| UpdateFailure::Busy)?;
    let page = candidate.page.ok_or(UpdateFailure::NoOffer)?;
    let launch = adapters(&window).launch;
    // Keep the gate until handoff settles. A new check cannot supersede
    // this already-approved action while the platform is opening the page.
    tauri::async_runtime::spawn_blocking(move || launch(page))
        .await
        .map_err(|_| UpdateFailure::Browser)??;
    candidate.page = None;
    Ok(())
}

#[tauri::command]
pub async fn verify_update_package<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    gate: tauri::State<'_, UpdateGate>,
) -> Result<Option<PackageVerification>, PackageFailure> {
    if window.label() != "main" {
        return Err(PackageFailure::NoMetadata);
    }
    let snapshot = gate.0.try_lock().map_err(|_| PackageFailure::Busy)?;
    let packages = snapshot
        .packages
        .as_ref()
        .ok_or(PackageFailure::NoMetadata)?
        .clone();
    let Some(path) = pick_package(&window).await? else {
        return Ok(None);
    };
    tauri::async_runtime::spawn_blocking(move || packages.verify(&path).map(Some))
        .await
        .map_err(|_| PackageFailure::ReadFailed)?
}

async fn pick_package<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Result<Option<PathBuf>, PackageFailure> {
    if let Some(picker) = adapters(window).package_picker {
        return Ok(picker());
    }
    let (send, receive) = tokio::sync::oneshot::channel();
    window
        .dialog()
        .file()
        .set_parent(window)
        .set_title("选择已下载的 Cumulus 安装包")
        .add_filter("Cumulus 安装包", &["deb", "gz", "zip", "exe"])
        .pick_file(move |selected| {
            let _ = send.send(selected);
        });
    let selected = receive.await.map_err(|_| PackageFailure::ReadFailed)?;
    selected
        .map(|file| file.into_path().map_err(|_| PackageFailure::InvalidFile))
        .transpose()
}
