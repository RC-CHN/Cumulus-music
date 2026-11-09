//! Focused main-window delivery only: never requests system notification permissions.
use cumulus_core::{
    Error, Result,
    client::{
        ClientError, MusicClient,
        daily_notification::{
            DailyNotificationCommand, DailyNotificationOutcome, DailyNotificationView,
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

pub fn start<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<()> {
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
    if matches!(action, DailyNotificationCommand::Disable) {
        let _ = window.emit_to(
            "main",
            PREFERENCE,
            serde_json::json!({"epoch":epoch,"value":false}),
        );
    }
    Ok(result)
}

#[tauri::command]
pub fn daily_notification_enabled<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> std::result::Result<bool, ClientError> {
    require_main(&window)?;
    Ok(client.daily_notification_enabled(&epoch)?)
}

#[tauri::command]
pub fn set_daily_notification_enabled<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    enabled: bool,
) -> std::result::Result<bool, ClientError> {
    require_main(&window)?;
    let enabled = client.set_daily_notification_enabled(&epoch, enabled)?;
    if !enabled {
        let _ = window.emit_to("main", EVENT, Option::<DailyNotificationView>::None);
    }
    let _ = window.emit_to(
        "main",
        PREFERENCE,
        serde_json::json!({"epoch":epoch,"value":enabled}),
    );
    Ok(enabled)
}
