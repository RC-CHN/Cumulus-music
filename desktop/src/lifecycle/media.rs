//! Native tray transport. No WebView relay and no decoder/network initialization
//! just to render a menu. Busy and stale targets cannot replay a write.
use super::*;
use cumulus_core::client::native_media::{NativeMediaAction, NativeMediaSnapshot};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
pub(super) struct Items<R: tauri::Runtime> {
    pub title: MenuItem<R>,
    pub previous: MenuItem<R>,
    pub toggle: MenuItem<R>,
    pub next: MenuItem<R>,
    pub stop: MenuItem<R>,
}
impl<R: tauri::Runtime> Items<R> {
    pub fn new(app: &tauri::AppHandle<R>) -> tauri::Result<Self> {
        Ok(Self {
            title: MenuItem::with_id(app, "media-title", "尚未选择音乐", false, None::<&str>)?,
            previous: MenuItem::with_id(app, "media-previous", "上一首", false, None::<&str>)?,
            toggle: MenuItem::with_id(app, "media-toggle", "播放", false, None::<&str>)?,
            next: MenuItem::with_id(app, "media-next", "下一首", false, None::<&str>)?,
            stop: MenuItem::with_id(app, "media-stop", "停止", false, None::<&str>)?,
        })
    }
    fn update(&self, value: &NativeMediaSnapshot, busy: bool) -> tauri::Result<()> {
        let title = if value.title.is_empty() {
            "尚未选择音乐".into()
        } else {
            value
                .title
                .chars()
                .filter(|c| !c.is_control())
                .take(60)
                .collect::<String>()
                .replace('&', "&&")
                .replace('_', "__")
        };
        self.title.set_text(title)?;
        self.toggle
            .set_text(if value.playing { "暂停" } else { "播放" })?;
        let enabled = value.target.is_some() && !busy;
        self.toggle.set_enabled(enabled)?;
        self.stop.set_enabled(enabled)?;
        self.previous.set_enabled(enabled && value.can_previous)?;
        self.next.set_enabled(enabled && value.can_next)
    }
}
struct MediaHost<R: tauri::Runtime> {
    current: Mutex<Option<(NativeMediaSnapshot, bool)>>,
    items: Mutex<Option<Arc<Items<R>>>>,
    busy: AtomicBool,
    stop: AtomicBool,
}
fn refresh<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    host: &Arc<MediaHost<R>>,
    completed: bool,
) {
    let handle = app.clone();
    let host = host.clone();
    let _ = app.run_on_main_thread(move || {
        if host.stop.load(Ordering::Acquire) {
            return;
        }
        let items = host.items.lock().ok().and_then(|items| items.clone());
        let Some(items) = items else {
            return;
        };
        // Read and publish in the same GUI callback as native menu updates.
        // No callback can see enabled controls with the preceding target.
        let value = handle
            .state::<MusicClient>()
            .native_media_snapshot()
            .unwrap_or_default();
        let busy = host.busy.load(Ordering::Acquire) && !completed;
        if let Ok(mut current) = host.current.lock()
            && current.as_ref() != Some(&(value.clone(), busy))
        {
            *current = if items.update(&value, busy).is_ok() {
                Some((value, busy))
            } else {
                None
            };
        }
        if completed {
            host.busy.store(false, Ordering::Release);
        }
    });
}
pub(super) fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>, items: Items<R>) {
    if app.try_state::<MusicClient>().is_none() {
        return;
    }
    let host = Arc::new(MediaHost {
        current: Mutex::new(None),
        items: Mutex::new(Some(Arc::new(items))),
        busy: AtomicBool::new(false),
        stop: AtomicBool::new(false),
    });
    app.manage(host.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while !host.stop.load(Ordering::Acquire) {
            refresh(&app, &host, false);
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    });
}
pub(super) fn stop<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(host) = app.try_state::<Arc<MediaHost<R>>>() {
        host.stop.store(true, Ordering::Release);
        // MenuItem retains AppHandle; break that ownership cycle on destruction.
        if let Ok(mut items) = host.items.lock() {
            items.take();
        }
    }
}
pub(super) fn activate<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: &str) {
    let action = match id {
        "media-previous" => NativeMediaAction::Previous,
        "media-toggle" => NativeMediaAction::Toggle,
        "media-next" => NativeMediaAction::Next,
        "media-stop" => NativeMediaAction::Stop,
        _ => return,
    };
    let Some(host) = app.try_state::<Arc<MediaHost<R>>>() else {
        return;
    };
    if host.stop.load(Ordering::Acquire)
        || host
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return;
    }
    let snapshot = host
        .current
        .lock()
        .ok()
        .and_then(|v| v.as_ref().map(|(value, _)| value.clone()));
    let Some(snapshot) = snapshot else {
        host.busy.store(false, Ordering::Release);
        return;
    };
    let Some(target) = snapshot.target.as_ref().cloned() else {
        host.busy.store(false, Ordering::Release);
        return;
    };
    // This handler is on the GUI thread. Disable before dispatch so a rapid
    // second click cannot use an enabled menu while its first action commits.
    let items = host.items.lock().ok().and_then(|items| items.clone());
    if let Some(items) = items {
        let _ = items.update(&snapshot, true);
    }
    if let Ok(mut current) = host.current.lock() {
        *current = Some((snapshot, true));
    }
    let host = host.inner().clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = app
            .state::<MusicClient>()
            .native_media_action(&target, action)
            .await;
        refresh(&app, &host, true);
        if result.is_err() && !host.stop.load(Ordering::Acquire) {
            show(&app);
            change(&app, |state| {
                state.view.message =
                    Some("播放状态已变化或操作未完成，请在播放器中确认后重试。".into())
            });
        }
    });
}
