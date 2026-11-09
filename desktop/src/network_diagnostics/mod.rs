mod route;

pub(crate) fn default_route() -> cumulus_core::network_diagnostics::DefaultRoute {
    route::default_route()
}

use cumulus_core::{
    Error,
    client::ClientError,
    network_diagnostics::{DefaultRoute, LocalNetworkReport, check_local_network},
};
use std::sync::Mutex;
use tauri::Manager;

/// Allows an embedding native host to supply its network route observation.
/// The production application leaves it unset and reads the system table.
#[doc(hidden)]
pub struct DefaultRouteObserver(Box<dyn Fn() -> DefaultRoute + Send + Sync>);

impl DefaultRouteObserver {
    pub fn new(observe: impl Fn() -> DefaultRoute + Send + Sync + 'static) -> Self {
        Self(Box::new(observe))
    }
}

pub(crate) fn observed_default_route<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> DefaultRoute {
    window
        .try_state::<DefaultRouteObserver>()
        .map_or_else(default_route, |observer| (observer.0)())
}

static CHECK: Mutex<()> = Mutex::new(());

#[tauri::command]
pub async fn local_network_diagnostics<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
) -> Result<LocalNetworkReport, ClientError> {
    if window.label() != "main" {
        return Err(Error::InvalidInput(
            "network diagnostics are only available in the main window",
        )
        .into());
    }
    tauri::async_runtime::spawn_blocking(|| {
        let _single = CHECK
            .try_lock()
            .map_err(|_| Error::State("network diagnostics are already running"))?;
        Ok::<_, Error>(check_local_network(default_route()))
    })
    .await
    .map_err(|_| Error::State("network diagnostics did not finish"))?
    .map_err(Into::into)
}
