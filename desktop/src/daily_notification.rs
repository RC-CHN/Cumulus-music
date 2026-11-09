//! Focused main-window delivery only: never requests system notification permissions.
use cumulus_core::{
    Error, Result,
    client::{
        ClientError, MusicClient,
        daily_notification::{
            DailyNotificationCommand, DailyNotificationKind, DailyNotificationOutcome,
            DailyNotificationView,
        },
    },
};
use std::{sync::Mutex, time::Duration};
use tauri::{Emitter, Manager};

const EVENT: &str = "cumulus:daily-notification";
const PREFERENCE: &str = "cumulus:daily-notification-preference";

#[derive(Default)]
pub(crate) struct Host(Mutex<Option<tauri::async_runtime::JoinHandle<()>>>);

fn foreground<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> bool {
    if !matches!(window.is_focused(), Ok(true))
        || !matches!(window.is_visible(), Ok(true))
        || !matches!(window.is_minimized(), Ok(false))
        || !matches!(window.is_fullscreen(), Ok(false))
    {
        return false;
    }
    // Official Windows uses the foreground RECT against the complete
    // monitor RECT. We can safely check that geometry when it is our window.
    let (Ok(position), Ok(size), Ok(Some(monitor))) = (
        window.outer_position(),
        window.outer_size(),
        window.current_monitor(),
    ) else {
        return false;
    };
    !(position == *monitor.position() && size == *monitor.size())
}

pub fn page_loaded<R: tauri::Runtime>(
    webview: &tauri::Webview<R>,
    page: &tauri::webview::PageLoadPayload<'_>,
) {
    if webview.label() != "main" || page.event() != tauri::webview::PageLoadEvent::Finished {
        return;
    }
    let app = webview.app_handle();
    let url = page.url();
    let bundled = matches!(
        (url.scheme(), url.host_str()),
        ("tauri", Some("localhost")) | ("http" | "https", Some("tauri.localhost"))
    ) && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none();
    let development = !cfg!(feature = "custom-protocol")
        && app
            .config()
            .build
            .dev_url
            .as_ref()
            .is_some_and(|dev| url.origin() == dev.origin());
    if (bundled || development) && app.try_state::<MusicClient>().is_some() && start(app).is_err() {
        eprintln!("Cumulus daily recommendation notification is unavailable.");
    }
}

fn start<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::ObjectExt;
        // Tao's outer_position is a cached Ok() even on Wayland, where the
        // compositor need not disclose absolute coordinates. Inspect GTK's
        // actual display (not the session environment), on the setup thread.
        if gtk::gdk::Display::default()
            .is_none_or(|display| display.type_().name() != "GdkX11Display")
        {
            return Ok(());
        }
    }
    let host = app.state::<Host>();
    let mut task = host.0.lock().map_err(|_| Error::LockPoisoned)?;
    if task.is_some() {
        return Ok(());
    }
    let app = app.clone();
    *task = Some(tauri::async_runtime::spawn(async move {
        // M/1384 invokes the notification initializer three seconds after startup.
        tokio::time::sleep(Duration::from_secs(3)).await;
        let client = app.state::<MusicClient>();
        let Ok(Some(schedule)) = client.daily_notification_startup_plan() else {
            return;
        };
        tokio::time::sleep(schedule.delay()).await;
        let Some(window) = app.get_webview_window("main") else {
            return;
        };
        if let Ok(Some(view)) = client
            .prepare_daily_notification(schedule, || foreground(&window))
            .await
        {
            let _ = window.emit_to("main", EVENT, Some(view));
        }
    }));
    Ok(())
}

pub fn stop<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(host) = app.try_state::<Host>()
        && let Ok(mut task) = host.0.lock()
        && let Some(task) = task.take()
    {
        task.abort();
    }
}

fn require_main<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> Result<()> {
    if window.label() != "main" {
        return Err(Error::InvalidInput(
            "daily notification requires the main window",
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn daily_notification<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> std::result::Result<Option<DailyNotificationView>, ClientError> {
    require_main(&window)?;
    if !foreground(&window) {
        return Ok(None);
    }
    Ok(client.current_daily_notification(&epoch)?)
}

#[tauri::command]
pub async fn daily_notification_command<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
    action: DailyNotificationCommand,
) -> std::result::Result<DailyNotificationOutcome, ClientError> {
    require_main(&window)?;
    let result = client
        .daily_notification_command(&epoch, &receipt, action, || foreground(&window))
        .await?;
    let disabled = match result {
        DailyNotificationOutcome::DisabledDaily => Some(DailyNotificationKind::Daily),
        DailyNotificationOutcome::DisabledCopyright => Some(DailyNotificationKind::Copyright),
        _ => None,
    };
    if let Some(kind) = disabled {
        let _ = window.emit_to(
            "main",
            PREFERENCE,
            serde_json::json!({"epoch":epoch,"kind":kind,"value":false}),
        );
    }
    Ok(result)
}

#[tauri::command]
pub fn daily_notification_enabled<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    kind: DailyNotificationKind,
) -> std::result::Result<Option<bool>, ClientError> {
    require_main(&window)?;
    Ok(client.daily_notification_enabled(&epoch, kind)?)
}

#[tauri::command]
pub fn set_daily_notification_enabled<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    kind: DailyNotificationKind,
    enabled: bool,
) -> std::result::Result<bool, ClientError> {
    require_main(&window)?;
    let enabled = client.set_daily_notification_enabled(&epoch, kind, enabled)?;
    let _ = window.emit_to(
        "main",
        PREFERENCE,
        serde_json::json!({"epoch":epoch,"kind":kind,"value":enabled}),
    );
    Ok(enabled)
}
