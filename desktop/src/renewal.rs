//! Foreground notification adapter; scheduling and credential decisions stay
//! in core. No periodic token rotation and no automatic mutation retry.
use cumulus_core::{
    account::refresh::{DesktopDeviceToken, DeviceInfo},
    client::{ClientError, MusicClient, dto::*},
    protocol::http::{ClientProfile, OFFICIAL_WINDOWS_USER_AGENT},
};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

#[derive(Default)]
pub(crate) struct RenewalPump {
    notify: Arc<tokio::sync::Notify>,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    observer: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

pub fn configure(client: MusicClient, profile: &ClientProfile) -> MusicClient {
    let os = if profile.user_agent == OFFICIAL_WINDOWS_USER_AGENT || cfg!(target_os = "windows") {
        "pc"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        std::env::consts::OS
    };
    // Official device.info uses getDeviceInfo().model, not the host name.
    // If this model is unavailable the official encoder yields "unknown".
    let model = model();
    client.with_official_login_policy().with_session_renewal(
        DeviceInfo {
            device_name: model,
            os: os.into(),
            client_type: Some(os.into()),
            ..Default::default()
        },
        Arc::new(DesktopDeviceToken),
    )
}

fn model() -> String {
    #[cfg(target_os = "linux")]
    {
        use std::io::Read;
        let mut text = String::new();
        if let Ok(file) = std::fs::File::open("/sys/class/dmi/id/product_name")
            && file.take(1025).read_to_string(&mut text).is_ok()
            && text.len() <= 1024
            && !text.contains('\0')
        {
            return text.trim().to_owned();
        }
    }
    String::new()
}

pub fn start<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let host = app.state::<RenewalPump>();
    let Ok(mut slot) = host.task.lock() else {
        return;
    };
    if slot.is_some() {
        return;
    }
    let notify = host.notify.clone();
    let app = app.clone();
    if let Ok(mut observer) = host.observer.lock()
        && let Ok(mut changes) = app.state::<MusicClient>().subscribe_account()
    {
        let app = app.clone();
        let notify = host.notify.clone();
        *observer = Some(tauri::async_runtime::spawn(async move {
            while changes.changed().await.is_ok() {
                // Publish account invalidation immediately even while an old
                // account's renewal/config request is still awaiting network.
                publish(&app, &app.state::<MusicClient>());
                notify.notify_one();
            }
        }));
    }
    *slot = Some(tauri::async_runtime::spawn(async move {
        let mut policy_tick = tokio::time::interval(std::time::Duration::from_secs(1800));
        policy_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        policy_tick.tick().await;
        loop {
            let foreground = tokio::select! {
                _ = notify.notified() => true,
                _ = policy_tick.tick() => false,
            };
            let client = app.state::<MusicClient>();
            let Ok(account) = client.account() else {
                continue;
            };
            if foreground {
                let _ = client
                    .renew_session(&account.epoch, RenewalAction::Check)
                    .await;
            } else {
                let _ = client.refresh_login_experiments(&account.epoch).await;
            }
            // Do not call auth::publish: it requests another check.
            publish(&app, &client);
        }
    }));
    host.notify.notify_one();
}

pub(crate) fn wake<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(host) = app.try_state::<RenewalPump>() {
        host.notify.notify_one();
    }
}
pub(crate) fn stop<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(host) = app.try_state::<RenewalPump>()
        && let Ok(mut observer) = host.observer.lock()
        && let Some(observer) = observer.take()
    {
        observer.abort();
    }
    if let Some(host) = app.try_state::<RenewalPump>()
        && let Ok(mut task) = host.task.lock()
        && let Some(task) = task.take()
    {
        task.abort();
    }
}
fn publish<R: tauri::Runtime>(app: &tauri::AppHandle<R>, client: &MusicClient) {
    if let Ok(account) = client.account() {
        if let Ok(view) = client.session_renewal(&account.epoch) {
            let _ = app.emit_to("main", "cumulus:session-renewal", view);
        }
        let _ = app.emit_to("main", "cumulus:account-changed", account);
    }
}

#[tauri::command]
pub fn session_renewal(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<SessionRenewalView, ClientError> {
    client.session_renewal(&epoch).map_err(Into::into)
}
#[tauri::command]
pub async fn renew_session<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    action: RenewalAction,
) -> Result<SessionRenewalView, ClientError> {
    let result = client
        .renew_session(&epoch, action)
        .await
        .map_err(Into::into);
    publish(window.app_handle(), &client);
    result
}
