//! Native ownership for foreground JSK and scheduled legacy renewal.
//! Scheduling, cohort checks and credential decisions stay in core.
use cumulus_core::{
    account::refresh::{DesktopDeviceToken, DeviceInfo, LegacyRenewal},
    client::{ClientError, MusicClient, dto::*},
    protocol::http::{ClientProfile, OFFICIAL_WINDOWS_USER_AGENT},
};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

#[cfg(any(windows, test))]
mod windows_focus;

#[derive(Default)]
pub(crate) struct RenewalPump {
    focus: Mutex<Foreground>,
    foreground: Arc<tokio::sync::Notify>,
    policy: Arc<tokio::sync::Notify>,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    observer: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    legacy: Mutex<Option<LegacyRenewal>>,
}

#[derive(Default)]
struct Foreground {
    focused: Option<bool>,
    pending_epoch: Option<String>,
}

impl Foreground {
    fn observe(&mut self, focused: bool, eligible: impl FnOnce() -> Option<String>) -> bool {
        if self.focused.replace(focused) == Some(focused) {
            return false;
        }
        self.pending_epoch = if focused { eligible() } else { None };
        self.pending_epoch.is_some()
    }
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
    #[cfg(windows)]
    if windows_focus::install(app).is_err() {
        eprintln!("Cumulus could not observe native window activation for session renewal.");
    }
    let foreground = host.foreground.clone();
    let policy = host.policy.clone();
    let app = app.clone();
    if let Ok(mut observer) = host.observer.lock()
        && let Ok(mut changes) = app.state::<MusicClient>().subscribe_account()
    {
        let app = app.clone();
        let policy = host.policy.clone();
        *observer = Some(tauri::async_runtime::spawn(async move {
            while changes.changed().await.is_ok() {
                // Publish account invalidation immediately even while an old
                // account's renewal/config request is still awaiting network.
                publish(&app, &app.state::<MusicClient>());
                policy.notify_one();
            }
        }));
    }
    *slot = Some(tauri::async_runtime::spawn(async move {
        let mut policy_tick = tokio::time::interval(std::time::Duration::from_secs(1800));
        policy_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        policy_tick.tick().await;
        loop {
            let from_focus = tokio::select! {
                _ = foreground.notified() => true,
                _ = policy.notified() => false,
                _ = policy_tick.tick() => false,
            };
            let client = app.state::<MusicClient>();
            if from_focus {
                let epoch = app
                    .state::<RenewalPump>()
                    .focus
                    .lock()
                    .ok()
                    .and_then(|mut focus| focus.pending_epoch.take());
                if let Some(epoch) = epoch
                    && client.foreground_renewal_epoch().ok().flatten().as_ref() == Some(&epoch)
                {
                    let _ = client.renew_session(&epoch, RenewalAction::Check).await;
                }
            } else if let Ok(account) = client.account() {
                let _ = client.refresh_login_experiments(&account.epoch).await;
            }
            publish(&app, &client);
        }
    }));
    host.policy.notify_one();
}

/// Native setup only, after managing the same policy-configured MusicClient
/// as the foreground JSK pump. Generic builders and other probes stay opt-in.
pub fn start_legacy<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> cumulus_core::Result<()> {
    let host = app.state::<RenewalPump>();
    let mut owner = host
        .legacy
        .lock()
        .map_err(|_| cumulus_core::Error::LockPoisoned)?;
    if owner.is_none() {
        let client = app.state::<MusicClient>();
        *owner = Some(tauri::async_runtime::block_on(async {
            client.start_legacy_session_renewal()
        })?);
    }
    Ok(())
}

pub(crate) fn focus_changed<R: tauri::Runtime>(app: &tauri::AppHandle<R>, focused: bool) {
    // M/1190 merges native focus and losefocus with distinctUntilChanged.
    // Never attach a later account or a later policy result to an old event.
    if let Some(host) = app.try_state::<RenewalPump>()
        && let Ok(mut focus) = host.focus.lock()
        && focus.observe(focused, || {
            app.try_state::<MusicClient>()?
                .foreground_renewal_epoch()
                .ok()
                .flatten()
        })
    {
        host.foreground.notify_one();
    }
}
pub(crate) fn stop<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(host) = app.try_state::<RenewalPump>()
        && let Ok(mut legacy) = host.legacy.lock()
    {
        drop(legacy.take());
    }
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
    if let Some(host) = app.try_state::<RenewalPump>()
        && let Ok(mut focus) = host.focus.lock()
    {
        *focus = Foreground::default();
    }
}

pub(crate) fn publish_status<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    client: &MusicClient,
    epoch: &str,
) {
    if let Ok(view) = client.session_renewal(epoch) {
        let _ = app.emit_to("main", "cumulus:session-renewal", view);
    }
}
fn publish<R: tauri::Runtime>(app: &tauri::AppHandle<R>, client: &MusicClient) {
    if let Ok(account) = client.account() {
        publish_status(app, client, &account.epoch);
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

#[cfg(test)]
mod tests {
    use super::Foreground;

    #[test]
    fn a_focus_edge_cannot_acquire_an_account_or_policy_after_it_arrives() {
        let mut foreground = Foreground::default();
        assert!(!foreground.observe(true, || None));
        assert!(!foreground.observe(true, || panic!("a duplicate focus is not an edge")));
        assert!(foreground.pending_epoch.is_none());

        assert!(!foreground.observe(false, || panic!("blur cannot check renewal policy")));
        assert!(foreground.observe(true, || Some("old".into())));
        assert!(!foreground.observe(true, || Some("new".into())));
        assert_eq!(foreground.pending_epoch.as_deref(), Some("old"));

        assert!(!foreground.observe(false, || None));
        assert!(
            foreground.pending_epoch.is_none(),
            "blur retires a queued check"
        );
        assert!(foreground.observe(true, || Some("new".into())));
        assert_eq!(foreground.pending_epoch.take().as_deref(), Some("new"));
    }
}
