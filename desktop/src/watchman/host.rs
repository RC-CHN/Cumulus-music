//! Formal native lifetime. Tokens/configuration never cross frontend IPC.
use super::collection::{BrowserMeasurements, MeasurementBridge};
use cumulus_core::{
    Error, Result,
    identity::{
        HardwareProfile, IdentityService, WindowsBrowserIdentity, seed_from_persistent_id,
        seed_from_state_root,
    },
    protocol::{
        context::RequestContext,
        http::ClientProfile,
        mconfig::{ConfigBackend, NativeConfigBackend, PcConfig},
    },
    storage::StateStore,
    watchman::{
        NativeBackend, NativeCollection, RefreshTask, SystemEntropyClock, Transport,
        WatchmanRuntime,
    },
};
use std::sync::{Arc, Mutex};
use tauri::Manager;
pub const PRODUCT: &str = "YD00000558929251";
struct Lifetime {
    closed: bool,
    refresh: Option<RefreshTask>,
}
pub struct NativeWatchman {
    runtime: Arc<WatchmanRuntime>,
    config: Arc<PcConfig>,
    context: Arc<RequestContext>,
    lifetime: Mutex<Lifetime>,
}
impl NativeWatchman {
    pub fn new<R: tauri::Runtime>(
        window: tauri::WebviewWindow<R>,
        store: Arc<StateStore>,
        profile: &ClientProfile,
        identity: Option<WindowsBrowserIdentity>,
        transport: Option<Arc<dyn Transport>>,
        config_backend: Option<Arc<dyn ConfigBackend>>,
    ) -> Result<Arc<Self>> {
        let source = Arc::new(BrowserMeasurements::new(window, identity)?);
        let transport = match transport {
            Some(t) => t,
            None => Arc::new(NativeBackend::new(&profile.user_agent)?),
        };
        let config_backend = match config_backend {
            Some(t) => t,
            None => Arc::new(NativeConfigBackend::new(&profile.user_agent)?),
        };
        let runtime = WatchmanRuntime::new(
            store.clone(),
            transport,
            Arc::new(SystemEntropyClock),
            Arc::new(NativeCollection::new(source)),
            PRODUCT,
        )?;
        let config = PcConfig::new(store, config_backend)?;
        let context = Arc::new(RequestContext::from_pc(
            config.clone(),
            profile.app_version(),
            Arc::new(runtime.clone()),
        )?);
        Ok(Arc::new(Self {
            runtime,
            config,
            context,
            lifetime: Mutex::new(Lifetime {
                closed: false,
                refresh: None,
            }),
        }))
    }
    pub fn context(&self) -> Arc<RequestContext> {
        self.context.clone()
    }
    pub fn runtime(&self) -> Arc<WatchmanRuntime> {
        self.runtime.clone()
    }
    pub fn pc_config(&self) -> Arc<PcConfig> {
        self.config.clone()
    }
    /// Run after the formal page-finished hook, inside the Tokio executor.
    pub fn start(&self) -> Result<()> {
        let mut state = self.lifetime.lock().map_err(|_| Error::LockPoisoned)?;
        if state.closed {
            return Err(Error::Cancelled);
        }
        if state.refresh.is_none() {
            state.refresh = Some(self.runtime.start_refresh());
        }
        Ok(())
    }
    pub fn close(&self) -> Result<()> {
        let mut state = self.lifetime.lock().map_err(|_| Error::LockPoisoned)?;
        state.closed = true;
        self.runtime.close()?;
        self.config.close()?;
        state.refresh.take();
        Ok(())
    }
}
impl Drop for NativeWatchman {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// The Windows client is what runs here on Linux, so the derived hardware picture
/// is on by default. `CUMULUS_WINDOWS_IDENTITY=off` restores declaration-only
/// behaviour, and a native Windows build keeps its real identity untouched.
/// A missing profile degrades to declarations instead of failing startup.
fn derive_identity(
    identity: WindowsBrowserIdentity,
    store: &Arc<StateStore>,
    root: &std::path::Path,
) -> WindowsBrowserIdentity {
    if cfg!(target_os = "windows")
        || matches!(
            std::env::var("CUMULUS_WINDOWS_IDENTITY").as_deref(),
            Ok("off") | Ok("0")
        )
    {
        return identity;
    }
    let language = identity.language().to_owned();
    let derive = || -> Result<HardwareProfile> {
        let seed = match IdentityService::new(store.clone()).stored_persistent_id()? {
            Some(id) => seed_from_persistent_id(&id)?,
            // Before the first identity write there is no installation ID yet;
            // the state directory keeps the choice stable until there is.
            None => seed_from_state_root(root),
        };
        HardwareProfile::derive(seed, &language)
    };
    match derive() {
        Ok(profile) => identity.with_hardware(profile),
        Err(error) => {
            eprintln!(
                "Cumulus: derived Windows hardware profile unavailable ({error}); continuing with declarations only"
            );
            identity
        }
    }
}

pub(crate) fn page_ready<R: tauri::Runtime>(webview: &tauri::Webview<R>) {
    if let Some(host) = webview.try_state::<Arc<NativeWatchman>>() {
        let host = host.inner().clone();
        tauri::async_runtime::spawn(async move {
            let _ = host.start();
        });
    }
}
pub fn install<R: tauri::Runtime>(
    app: &tauri::App<R>,
    root: &std::path::Path,
    profile: &ClientProfile,
) -> Result<Arc<RequestContext>> {
    let window = app
        .get_webview_window("main")
        .ok_or(Error::State("main window unavailable"))?;
    let store = Arc::new(StateStore::open(root)?);
    let identity =
        if profile.user_agent == cumulus_core::protocol::http::OFFICIAL_WINDOWS_USER_AGENT {
            let locales = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
                .into_iter()
                .find_map(|k| std::env::var(k).ok().filter(|s| !s.is_empty()))
                .unwrap_or_default();
            let identity = WindowsBrowserIdentity::from_locales(
                &locales.split(':').map(str::to_owned).collect::<Vec<_>>(),
                "zh-CN",
            )?;
            Some(derive_identity(identity, &store, root))
        } else {
            None
        };
    let host = NativeWatchman::new(window.clone(), store, profile, identity, None, None)?;
    let context = host.context();
    app.manage(host.clone());
    if window.state::<MeasurementBridge>().epoch().is_ok() {
        tauri::async_runtime::spawn(async move {
            let _ = host.start();
        });
    }
    Ok(context)
}
