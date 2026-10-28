//! Resource likes use the core journal and remain separate from transport.
use super::*;
use cumulus_core::client::native_media::{
    NativeLikeResource, NativeLikeSelection, NativeLikeSnapshot,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
const LIKE_ERROR: &str = "喜欢操作尚未确认，请使用托盘的刷新或核对入口读取状态。";
pub(super) struct Items<R: tauri::Runtime> {
    pub toggle: MenuItem<R>,
    pub refresh: MenuItem<R>,
}
impl<R: tauri::Runtime> Items<R> {
    pub fn new(app: &tauri::AppHandle<R>) -> tauri::Result<Self> {
        Ok(Self {
            toggle: MenuItem::with_id(app, "native-like", "喜欢", false, None::<&str>)?,
            refresh: MenuItem::with_id(
                app,
                "native-like-refresh",
                "刷新喜欢状态",
                false,
                None::<&str>,
            )?,
        })
    }
    fn update(&self, value: &View) -> tauri::Result<()> {
        let pending = value.snapshot.as_ref().is_some_and(|v| v.pending);
        let observed = value.snapshot.as_ref().and_then(|v| v.observed);
        self.toggle.set_text(if pending {
            "喜欢状态待核对"
        } else if value.selection.is_some() && observed.is_none() {
            "喜欢状态未读取"
        } else if observed == Some(true) {
            "取消喜欢"
        } else {
            "喜欢"
        })?;
        self.toggle
            .set_enabled(!value.loading && !pending && observed.is_some())?;
        self.refresh.set_text(if pending {
            "核对喜欢状态"
        } else {
            "刷新喜欢状态"
        })?;
        self.refresh
            .set_enabled(!value.loading && value.selection.is_some())
    }
}
#[derive(Clone, Default, PartialEq, Eq)]
struct View {
    selection: Option<NativeLikeSelection>,
    snapshot: Option<NativeLikeSnapshot>,
    loading: bool,
    generation: u64,
}
struct Host<R: tauri::Runtime> {
    items: Mutex<Option<Arc<Items<R>>>>,
    view: Mutex<View>,
    busy: AtomicBool,
    stop: AtomicBool,
    generation: AtomicU64,
}
#[derive(Clone, Serialize)]
struct Changed {
    epoch: String,
    resource: NativeLikeResource,
}
fn changed<R: tauri::Runtime>(app: &tauri::AppHandle<R>, target: &NativeLikeSelection) {
    let _ = app.emit_to(
        "main",
        "cumulus:native-like-changed",
        Changed {
            epoch: target.epoch(),
            resource: target.resource.clone(),
        },
    );
}
fn publish<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    host: &Arc<Host<R>>,
    mut value: View,
    completed: bool,
) {
    let app_copy = app.clone();
    let host = host.clone();
    let _ = app.run_on_main_thread(move || {
        if host.stop.load(Ordering::Acquire)
            || value.generation != host.generation.load(Ordering::Acquire)
        {
            return;
        }
        if app_copy
            .state::<MusicClient>()
            .native_like_selection()
            .ok()
            .flatten()
            != value.selection
        {
            value = View::default();
        }
        if host.busy.load(Ordering::Acquire) && !completed {
            value.loading = true;
        }
        let items = host.items.lock().ok().and_then(|v| v.clone());
        if let Some(items) = items
            && let Ok(mut current) = host.view.lock()
            && *current != value
        {
            *current = if items.update(&value).is_ok() {
                value
            } else {
                View::default()
            };
        }
        if completed {
            host.busy.store(false, Ordering::Release);
        }
    });
}
fn local<R: tauri::Runtime>(app: &tauri::AppHandle<R>, generation: u64) -> View {
    let client = app.state::<MusicClient>();
    let selection = client.native_like_selection().ok().flatten();
    let snapshot = selection
        .as_ref()
        .and_then(|s| client.native_like_snapshot(s).ok());
    View {
        selection,
        snapshot,
        loading: false,
        generation,
    }
}
pub(super) fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>, items: Items<R>) {
    if app.try_state::<MusicClient>().is_none() {
        return;
    }
    let host = Arc::new(Host {
        items: Mutex::new(Some(Arc::new(items))),
        view: Mutex::new(View::default()),
        busy: AtomicBool::new(false),
        stop: AtomicBool::new(false),
        generation: AtomicU64::new(0),
    });
    app.manage(host.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut attempted = None;
        while !host.stop.load(Ordering::Acquire) {
            if !host.busy.load(Ordering::Acquire) {
                let generation = host.generation.load(Ordering::Acquire);
                let value = local(&app, generation);
                if attempted != value.selection {
                    attempted = value.selection.clone();
                    if let Some(target) = value.selection.as_ref()
                        && value.snapshot.as_ref().is_none_or(|v| v.observed.is_none())
                        && host
                            .busy
                            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                            .is_ok()
                    {
                        publish(
                            &app,
                            &host,
                            View {
                                loading: true,
                                ..value.clone()
                            },
                            false,
                        );
                        let _ = tokio::time::timeout(
                            Duration::from_secs(30),
                            app.state::<MusicClient>().native_like_refresh(target),
                        )
                        .await;
                        publish(&app, &host, local(&app, generation), true);
                    } else {
                        publish(&app, &host, value, false);
                    }
                } else {
                    publish(&app, &host, value, false);
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });
}
pub(super) fn stop<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(host) = app.try_state::<Arc<Host<R>>>() {
        host.stop.store(true, Ordering::Release);
        if let Ok(mut items) = host.items.lock() {
            items.take();
        }
    }
}
pub(super) fn activate<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: &str) {
    if !matches!(id, "native-like" | "native-like-refresh") {
        return;
    }
    let refresh = id == "native-like-refresh";
    let Some(host) = app.try_state::<Arc<Host<R>>>() else {
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
    let value = host.view.lock().ok().map(|v| v.clone()).unwrap_or_default();
    let Some(target) = value.selection.clone() else {
        host.busy.store(false, Ordering::Release);
        return;
    };
    // Immediate GUI disable, followed by a fresh target/menu update at completion.
    if let Some(items) = host.items.lock().ok().and_then(|v| v.clone()) {
        let _ = items.update(&View {
            loading: true,
            ..value.clone()
        });
    }
    let generation = host.generation.fetch_add(1, Ordering::AcqRel) + 1;
    let host = host.inner().clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let client = app.state::<MusicClient>();
        let result = if refresh {
            client.native_like_refresh(&target).await.and_then(|value| {
                if value.pending {
                    Err(cumulus_core::Error::MutationUncertain)
                } else {
                    Ok(())
                }
            })
        } else {
            match value.snapshot {
                Some(view) => client.native_like_toggle(&view).await,
                None => Err(cumulus_core::Error::State("like state is unknown")),
            }
        };
        // Even a lost reply may have changed the service. This event requests
        // read-back only; it never instructs the frontend to repeat the write.
        changed(&app, &target);
        publish(&app, &host, local(&app, generation), true);
        if result.is_ok() && view(&app).is_ok_and(|v| v.message.as_deref() == Some(LIKE_ERROR)) {
            change(&app, |state| {
                if state.view.message.as_deref() == Some(LIKE_ERROR) {
                    state.view.message = None;
                }
            });
        }
        if result.is_err() && !host.stop.load(Ordering::Acquire) {
            show(&app);
            change(&app, |state| state.view.message = Some(LIKE_ERROR.into()));
        }
    });
}
