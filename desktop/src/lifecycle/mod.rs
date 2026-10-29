//! Native window lifecycle. Installed only after the business client exists;
//! hiding never performs logout, room cleanup, media changes or remote writes.
mod actions;
mod availability;
mod likes;
mod media;
use cumulus_core::{
    client::{MusicClient, dto::WindowPreferencesView},
    preferences::window::CloseBehavior,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
const TRAY_ID: &str = "cumulus-main";
const EVENT: &str = "cumulus:window-lifecycle";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosePrompt {
    pub id: String,
    pub epoch: String,
    pub preferences: WindowPreferencesView,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleView {
    pub revision: String,
    pub installed: bool,
    pub tray_available: bool,
    pub prompt: Option<ClosePrompt>,
    pub busy: bool,
    pub hidden: bool,
    pub message: Option<String>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CloseCommand {
    Cancel {
        id: String,
    },
    Confirm {
        id: String,
        behavior: CloseBehavior,
        remember: bool,
    },
}
#[derive(Default)]
struct Inner {
    revision: u64,
    operation: u64,
    view: LifecycleView,
    checked: Option<Instant>,
    exiting: bool,
}
impl Inner {
    fn available(&self) -> bool {
        self.view.tray_available
            && self
                .checked
                .is_some_and(|time| time.elapsed() < Duration::from_secs(4))
    }
    fn snapshot(&self) -> LifecycleView {
        LifecycleView {
            revision: self.revision.to_string(),
            tray_available: self.available(),
            ..self.view.clone()
        }
    }
    fn next(&mut self) -> u64 {
        // Process-local counters cannot be reached by frontend input.
        self.operation = self
            .operation
            .checked_add(1)
            .expect("window lifecycle exhausted");
        self.operation
    }
}
struct Host {
    inner: Mutex<Inner>,
    monitor: Mutex<Option<availability::Monitor>>,
    #[cfg(target_os = "linux")]
    _library: Option<libloading::Library>,
}
fn change<R: tauri::Runtime>(app: &tauri::AppHandle<R>, f: impl FnOnce(&mut Inner)) {
    let Some(host) = app.try_state::<Host>() else {
        return;
    };
    let Ok(mut inner) = host.inner.lock() else {
        return;
    };
    f(&mut inner);
    inner.revision = inner.revision.saturating_add(1);
    let snapshot = inner.snapshot();
    drop(inner);
    let _ = app.emit_to("main", EVENT, snapshot);
}
pub fn view<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<LifecycleView, String> {
    let Some(host) = app.try_state::<Host>() else {
        return Ok(LifecycleView {
            revision: "0".into(),
            ..Default::default()
        });
    };
    host.inner
        .lock()
        .map(|state| state.snapshot())
        .map_err(|_| "无法读取窗口状态".into())
}
fn availability<R: tauri::Runtime>(app: &tauri::AppHandle<R>, available: bool) {
    let Some(host) = app.try_state::<Host>() else {
        return;
    };
    let Ok(mut state) = host.inner.lock() else {
        return;
    };
    // A fresh success also revives a previously expired observation. Publish
    // that transition even if the last raw boolean was already true.
    let changed = state.available() != available;
    state.checked = Some(Instant::now());
    let restore = !available && state.view.hidden;
    state.view.tray_available = available;
    if restore {
        state.view.hidden = false;
        state.view.busy = false;
        state.next();
        state.view.message = Some("系统托盘暂不可用，已恢复主窗口。".into());
    }
    drop(state);
    if restore {
        reveal(app);
    }
    if changed || restore {
        change(app, |_| {});
    }
}
fn reveal<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if !crate::mini::dismiss(app, None, true) {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
pub fn show<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    change(app, |state| {
        if !state.exiting {
            state.next();
            state.view.busy = false;
        }
        state.view.hidden = false;
    });
    reveal(app);
}
pub(crate) fn stop<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(host) = app.try_state::<Host>()
        && let Ok(mut monitor) = host.monitor.lock()
    {
        monitor.take();
    }
    likes::stop(app);
    media::stop(app);
    app.remove_tray_by_id(TRAY_ID);
}
/// Calls must originate on the native GUI thread. A missing tray library or
/// panel is a recoverable capability failure, never a fatal app startup error.
pub fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    if app.try_state::<Host>().is_some() {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    let library = availability::library();
    #[cfg(target_os = "linux")]
    let native_available = library.is_some();
    #[cfg(not(target_os = "linux"))]
    let native_available = true;
    app.manage(Host {
        inner: Mutex::new(Inner {
            view: LifecycleView {
                installed: true,
                ..Default::default()
            },
            ..Default::default()
        }),
        monitor: Mutex::new(None),
        #[cfg(target_os = "linux")]
        _library: library,
    });
    if !native_available {
        return Ok(());
    }
    // Keep construction failures inside the capability boundary. The window
    // close prompt remains usable with an explicit Exit choice.
    if let Err(error) = build(app) {
        eprintln!("native tray unavailable: {error}");
    }
    Ok(())
}
fn build<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "cumulus-show", "显示 Cumulus", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "cumulus-quit", "退出 Cumulus", true, None::<&str>)?;
    let unlock_lyrics = MenuItem::with_id(
        app,
        "cumulus-lyric-unlock",
        "解锁桌面歌词",
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let media = media::Items::new(app)?;
    let likes = likes::Items::new(app)?;
    let transport_separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &show_item,
            &separator,
            &media.title,
            &media.previous,
            &media.toggle,
            &media.next,
            &media.stop,
            &likes.toggle,
            &likes.refresh,
            &transport_separator,
            &unlock_lyrics,
            &quit,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Cumulus 云间音乐")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "cumulus-show" => show(app),
            "cumulus-quit" => actions::request(app, true),
            "cumulus-lyric-unlock" => {
                if crate::desktop_lyrics::unlock(app).is_err() {
                    show(app);
                }
            }
            _ => {
                media::activate(app, event.id.as_ref());
                likes::activate(app, event.id.as_ref());
            }
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let tray = builder.build(app)?;
    media::install(app, media);
    likes::install(app, likes);
    let observed = app.clone();
    #[cfg(target_os = "linux")]
    let monitor = availability::monitor(tray.id().as_ref(), move |ok| availability(&observed, ok));
    #[cfg(target_os = "windows")]
    let monitor = availability::monitor(tray, move |ok| availability(&observed, ok));
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if let Ok(mut guard) = app.state::<Host>().monitor.lock() {
        *guard = Some(monitor);
    }
    Ok(())
}
pub(crate) fn closing<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    event: &tauri::WindowEvent,
) -> bool {
    if window.label() != "main" || window.try_state::<Host>().is_none() {
        return false;
    }
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        actions::request(window.app_handle(), false);
        return true;
    }
    false
}
#[tauri::command]
pub async fn window_lifecycle<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<LifecycleView, String> {
    view(&app)
}
#[tauri::command]
pub async fn window_close_command<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    command: CloseCommand,
) -> Result<LifecycleView, String> {
    actions::command(&app, command).await?;
    view(&app)
}
