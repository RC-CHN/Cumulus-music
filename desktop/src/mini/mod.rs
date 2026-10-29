//! Lightweight second WebView; all media work stays in the shared MusicClient.
pub(crate) mod commands;
mod shortcuts;
mod state;
use cumulus_core::{Error, Result, client::MusicClient};
pub(crate) use shortcuts::{ModeIntent, capture_mode, switch_mode};
use state::{Ledger, Phase};
use std::sync::Mutex;
use tauri::{Manager, Runtime, webview::PageLoadEvent};

#[derive(Default)]
pub(crate) struct Host {
    ledger: Mutex<Ledger>,
    operation: tokio::sync::Mutex<()>,
}
impl Host {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Ledger>> {
        self.ledger
            .lock()
            .map_err(|_| Error::State("mini host unavailable"))
    }
}
pub(crate) fn owns<R: Runtime>(app: &tauri::AppHandle<R>, label: &str) -> bool {
    app.try_state::<Host>()
        .and_then(|h| {
            h.lock()
                .ok()
                .map(|l| l.window.as_ref().is_some_and(|w| w.label == label))
        })
        .unwrap_or(false)
}
async fn gui<R: Runtime, T: Send + 'static>(
    app: &tauri::AppHandle<R>,
    action: impl FnOnce(tauri::AppHandle<R>) -> Result<T> + Send + 'static,
) -> Result<T> {
    let (send, receive) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = send.send(action(handle));
    })
    .map_err(|_| Error::State("mini GUI unavailable"))?;
    receive
        .await
        .map_err(|_| Error::State("mini GUI unavailable"))?
}
fn main_visible<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<()> {
    let main = app
        .get_webview_window("main")
        .ok_or(Error::State("main window unavailable"))?;
    main.show()
        .and_then(|_| main.unminimize())
        .map_err(|_| Error::State("main window unavailable"))?;
    let _ = main.set_focus();
    Ok(())
}
/// Reveal first; never destroy the user's remaining surface when restoration
/// fails. Revoke the lifetime before Destroyed callbacks can run.
pub(crate) fn dismiss<R: Runtime>(
    app: &tauri::AppHandle<R>,
    generation: Option<&str>,
    reveal: bool,
) -> bool {
    let current = app.try_state::<Host>().and_then(|h| {
        h.lock().ok().and_then(|l| {
            l.window
                .as_ref()
                .filter(|w| generation.is_none_or(|g| g == w.generation))
                .map(|w| w.generation.clone())
        })
    });
    let Some(current) = current else {
        return true;
    };
    if reveal && main_visible(app).is_err() {
        return false;
    }
    let window = app
        .try_state::<Host>()
        .and_then(|h| h.lock().ok().and_then(|mut l| l.take(Some(&current))));
    if let Some(window) = window {
        crate::shortcuts::local::closed(app, &window.label);
        if let Some(window) = app.get_webview_window(&window.label) {
            let _ = window.destroy();
        }
    }
    true
}
pub async fn enter<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<()> {
    enter_expected(app, None).await
}
async fn enter_expected<R: Runtime>(
    app: &tauri::AppHandle<R>,
    intent: Option<ModeIntent>,
) -> Result<()> {
    let worker = app.clone();
    let preferences = tauri::async_runtime::spawn_blocking(move || {
        worker.state::<MusicClient>().mini_preferences_native()
    })
    .await
    .map_err(|_| Error::State("mini settings unavailable"))??;
    let (generation, label) = gui(app, move |app| {
        if let Some(intent) = &intent {
            intent.check(&app)?;
        }
        let lifecycle = crate::lifecycle::view(&app)
            .map_err(|_| Error::State("window lifecycle unavailable"))?;
        if lifecycle.busy || lifecycle.prompt.is_some() {
            return Err(Error::MutationBusy);
        }
        let epoch = if let Some(intent) = &intent {
            intent.epoch()
        } else {
            app.state::<MusicClient>().account()?.epoch
        };
        app.state::<Host>().lock()?.begin(epoch)
    })
    .await?;
    let worker = app.clone();
    let building_generation = generation.clone();
    // WebView2 forbids creating another WebView inside a synchronous event
    // handler. Let Tauri marshal construction from a worker, on both platforms.
    let building = tauri::async_runtime::spawn_blocking(move || -> Result<()> {
        worker.state::<Host>().lock()?.get(&building_generation)?;
        let url = format!("index.html#mini={building_generation}");
        let loaded_generation = building_generation.clone();
        let window =
            tauri::WebviewWindowBuilder::new(&worker, label, tauri::WebviewUrl::App(url.into()))
                .title("Cumulus · 迷你播放器")
                .inner_size(420.0, 206.0)
                .decorations(false)
                .resizable(false)
                .maximizable(false)
                .focused(false)
                .always_on_top(preferences.always_on_top && !preferences.damaged)
                .on_page_load(move |webview, event| {
                    if event.event() != PageLoadEvent::Started {
                        return;
                    }
                    let reload = webview
                        .state::<Host>()
                        .lock()
                        .ok()
                        .and_then(|mut ledger| {
                            ledger.get_mut(&loaded_generation).ok().map(|w| {
                                let loaded = w.loaded;
                                w.loaded = true;
                                loaded
                            })
                        })
                        .unwrap_or(false);
                    if reload {
                        dismiss(webview.app_handle(), Some(&loaded_generation), true);
                    }
                })
                .build()
                .map_err(|_| Error::State("mini window creation failed"))?;
        if worker
            .state::<Host>()
            .lock()?
            .get(&building_generation)
            .is_err()
        {
            // A timeout/close can retire the lifetime during platform creation.
            // Destroy this late window without touching a newer mini instance.
            let _ = window.destroy();
            return Err(Error::StaleResponse);
        }
        Ok(())
    });
    let result = tokio::time::timeout(std::time::Duration::from_secs(8), async {
        building
            .await
            .map_err(|_| Error::State("mini window creation failed"))??;
        loop {
            let phase = app.state::<Host>().lock()?.get(&generation)?.phase;
            if phase == Phase::Active {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        }
    })
    .await
    .unwrap_or(Err(Error::State("mini startup timed out")));
    if result.is_err() {
        gui(app, move |app| {
            dismiss(&app, Some(&generation), true);
            Ok(())
        })
        .await?;
    }
    result
}

pub(crate) fn closing<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) -> bool {
    if !owns(window.app_handle(), window.label()) {
        return false;
    }
    match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            if dismiss(window.app_handle(), None, true)
                && let Some(main) = window.app_handle().get_webview_window("main")
            {
                let _ = main.close();
            }
        }
        tauri::WindowEvent::Destroyed => {
            dismiss(window.app_handle(), None, true);
        }
        _ => (),
    }
    true
}
