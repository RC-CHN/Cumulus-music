use cumulus_core::updates::{
    GitHubReleases, LatestReleaseSource, ReleasePage, UpdateCheck, UpdateFailure, check_latest,
};
use std::sync::Arc;
use tauri::Manager;

#[derive(Default)]
pub(crate) struct UpdateGate(tokio::sync::Mutex<Option<ReleasePage>>);

#[derive(Clone)]
pub struct UpdateAdapters {
    source: Arc<dyn LatestReleaseSource>,
    launch: Arc<dyn Fn(ReleasePage) -> Result<(), UpdateFailure> + Send + Sync>,
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
        }
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
    *candidate = None;
    let result = check_latest(env!("CARGO_PKG_VERSION"), adapters(&window).source.as_ref()).await?;
    *candidate = result.release;
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
    let page = candidate.ok_or(UpdateFailure::NoOffer)?;
    let launch = adapters(&window).launch;
    // Keep the gate until handoff settles. A new check cannot supersede
    // this already-approved action while the platform is opening the page.
    tauri::async_runtime::spawn_blocking(move || launch(page))
        .await
        .map_err(|_| UpdateFailure::Browser)??;
    *candidate = None;
    Ok(())
}
