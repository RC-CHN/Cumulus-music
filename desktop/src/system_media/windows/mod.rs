//! Manual WinRT SMTC integration for the libmpv player. No MediaPlayer decoder.
mod commands;
mod events;
mod metadata;
mod native;
use cumulus_core::client::{MusicClient, native_media::NativeMediaDetails};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use tauri::Manager;
use windows::Media::SystemMediaTransportControls;

struct Host {
    stopped: AtomicBool,
    busy: AtomicBool,
    refresh_controls: AtomicBool,
    generation: AtomicU64,
    published: Mutex<Option<NativeMediaDetails>>,
    wake: mpsc::SyncSender<()>,
}
pub(super) fn stop<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(host) = app.try_state::<Arc<Host>>() {
        host.stopped.store(true, Ordering::Release);
        host.generation.fetch_add(1, Ordering::AcqRel);
        let _ = host.wake.try_send(());
    }
}
pub(super) fn install<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if app.try_state::<Arc<Host>>().is_some() || app.try_state::<MusicClient>().is_none() {
        return;
    }
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(window) = window.hwnd() else { return };
    let (wake, requests) = mpsc::sync_channel(1);
    let host = Arc::new(Host {
        stopped: AtomicBool::new(false),
        busy: AtomicBool::new(false),
        refresh_controls: AtomicBool::new(false),
        generation: AtomicU64::new(0),
        published: Mutex::new(None),
        wake,
    });
    app.manage(host.clone());
    let app = app.clone();
    let hwnd = window.0 as usize;
    // One dedicated MTA owns creation, metadata and revocation. WinRT callbacks
    // only enqueue fenced core work and never block the Tauri window thread.
    if std::thread::Builder::new()
        .name("cumulus-smtc".into())
        .spawn(move || {
            let Ok(_apartment) = native::Apartment::open() else {
                eprintln!("Windows media integration unavailable");
                return;
            };
            run(&app, &host, hwnd, requests);
        })
        .is_err()
    {
        eprintln!("Windows media worker unavailable");
    }
}
struct Bridge {
    controls: SystemMediaTransportControls,
    subscriptions: Vec<events::Subscription>,
    previous: Option<NativeMediaDetails>,
    timeline_at: Instant,
}
impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.controls.SetIsEnabled(false);
        for subscription in self.subscriptions.drain(..) {
            subscription.remove(&self.controls);
        }
        if let Ok(display) = self.controls.DisplayUpdater() {
            let _ = display.ClearAll();
            let _ = display.Update();
        }
    }
}
fn retire(host: &Host, bridge: &mut Option<Bridge>) {
    host.generation.fetch_add(1, Ordering::AcqRel);
    if let Ok(mut view) = host.published.lock() {
        *view = None;
    }
    *bridge = None;
}
fn run<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    host: &Arc<Host>,
    hwnd: usize,
    requests: mpsc::Receiver<()>,
) {
    let mut bridge: Option<Bridge> = None;
    let mut retry = Instant::now();
    while !host.stopped.load(Ordering::Acquire) {
        if let Ok(view) = app.state::<MusicClient>().native_media_details() {
            if view.transport.target.is_none() {
                if bridge.is_some() {
                    retire(host, &mut bridge);
                }
            } else {
                if bridge.is_none() && Instant::now() >= retry {
                    let generation = host.generation.fetch_add(1, Ordering::AcqRel) + 1;
                    bridge = native::open(hwnd)
                        .and_then(|controls| {
                            let mut result = Bridge {
                                controls,
                                subscriptions: vec![],
                                previous: None,
                                timeline_at: Instant::now(),
                            };
                            events::install(app, host, generation, &mut result)?;
                            Ok(result)
                        })
                        .map_err(|error| {
                            eprintln!("Windows media integration unavailable: {}", error.code());
                        })
                        .ok();
                    retry = Instant::now() + Duration::from_secs(2);
                }
                if let Some(current) = bridge.as_mut() {
                    match metadata::publish(
                        current,
                        &view,
                        host.refresh_controls.swap(false, Ordering::AcqRel),
                    ) {
                        Ok(()) => {
                            if let Ok(mut published) = host.published.lock() {
                                *published = Some(view);
                            }
                        }
                        Err(error) => {
                            eprintln!("Windows media update failed: {}", error.code());
                            retire(host, &mut bridge);
                            retry = Instant::now() + Duration::from_secs(2);
                        }
                    }
                }
            }
        } else if bridge.is_some() {
            retire(host, &mut bridge);
        }
        let _ = requests.recv_timeout(Duration::from_millis(250));
    }
    retire(host, &mut bridge);
}
