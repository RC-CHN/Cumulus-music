//! A rendered local splash only: no account, music client, filesystem IPC or network.
use super::policy::Outcome;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Manager, webview::PageLoadEvent};

pub(super) const ARG: &str = "--cumulus-renderer-check";
pub(super) const CANCELLED: i32 = 125;
pub(super) const TRACE_ENV: &str = "CUMULUS_RENDERER_TRACE";

pub(super) fn run(root: &Path) -> i32 {
    let diagnostic = std::env::var_os(TRACE_ENV).is_some();
    let started = std::time::Instant::now();
    let trace = move |stage: &str| {
        if diagnostic {
            eprintln!(
                "renderer probe +{}ms: {stage}",
                started.elapsed().as_millis()
            );
        }
    };
    trace("waiting for parent");
    // The Windows parent attaches a kill-on-close job before allowing WebView2
    // children to start. A crashed parent cannot leave an unbounded waiter.
    let waiting = std::time::Instant::now();
    while !root.join("start").exists() {
        if waiting.elapsed() >= std::time::Duration::from_secs(5) {
            return 1;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // SAFETY: this is the child process entry, before any runtime or worker.
    unsafe { super::prepare_process() };
    trace("initializing WebView");
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    let report = root.join("ready");
    let profile = root.join("webview");
    let reported = Arc::new(AtomicBool::new(false));
    let complete = reported.clone();
    let result = tauri::Builder::default()
        .plugin(super::plugin())
        .register_uri_scheme_protocol("cumulus-renderer", move |_, _| {
            trace("serving local splash response");
            tauri::http::Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                .header("Content-Security-Policy", "default-src 'none'; style-src 'unsafe-inline'; connect-src ipc: http://ipc.localhost")
                .body(include_bytes!("probe.html").to_vec()).unwrap()
        })
        .invoke_handler(move |invoke| {
            if invoke.message.webview_ref().label() == "main" && invoke.message.command() == "renderer_cancel" {
                invoke.message.webview_ref().app_handle().exit(CANCELLED);
                invoke.resolver.resolve(());
                return true;
            }
            if invoke.message.webview_ref().label() != "main" || invoke.message.command() != "renderer_ready" {
                invoke.resolver.reject("Graphics check only");
                return true;
            }
            if complete.swap(true, Ordering::AcqRel) {
                invoke.resolver.resolve(());
                return true;
            }
            trace("animation acknowledged");
            let webview = invoke.message.webview();
            let handle = webview.app_handle().clone();
            let output = report.clone();
            invoke.resolver.resolve(());
            #[cfg(target_os = "linux")]
            {
                let fallback = handle.clone();
                if webview.with_webview(move |view| {
                    use webkit2gtk::WebViewExt;
                    view.inner().snapshot(webkit2gtk::SnapshotRegion::Visible,
                        webkit2gtk::SnapshotOptions::NONE, None::<&webkit2gtk::gio::Cancellable>, move |result| {
                        trace("snapshot returned");
                        let ok = result.is_ok_and(|surface| {
                            let Ok(image) = surface.map_to_image(None) else { return false; };
                            if image.width() < 100 || image.height() < 100 { return false; }
                            let mut colored = 0;
                            image.with_data(|data| {
                                // Cairo ARGB32/RGB24 uses native-endian 0xAARRGGBB.
                                // The splash has a pink logo; blank surfaces cannot pass.
                                colored = data.chunks_exact(4).filter(|pixel| {
                                    let rgb = u32::from_ne_bytes((*pixel).try_into().unwrap());
                                    ((rgb >> 16) & 255) > 220 && ((rgb >> 8) & 255) < 100 && (rgb & 255) < 150
                                }).count();
                            }).is_ok() && colored > 100
                        });
                        finish(&handle, &output, ok);
                    });
                }).is_err() { fallback.exit(1); }
            }
            #[cfg(not(target_os = "linux"))]
            finish(&handle, &output, true);
            true
        })
        .setup(move |app| {
            trace("building isolated WebView window");
            let window = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::CustomProtocol("cumulus-renderer://localhost".parse()?))
                .title("Cumulus").inner_size(360.0, 220.0).resizable(false)
                .decorations(false).center().data_directory(profile)
                .on_navigation(move |_| { trace("navigation requested"); true })
                .build()?;
            trace("WebView created");
            #[cfg(target_os = "linux")]
            {
                let handle = app.handle().clone();
                window.with_webview(move |view| {
                    use webkit2gtk::WebViewExt;
                    view.inner().connect_web_process_terminated(move |_, _| {
                        trace("Web process terminated");
                        handle.exit(1);
                    });
                })?;
            }
            #[cfg(not(target_os = "linux"))]
            let _ = window;
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                window.app_handle().exit(CANCELLED);
            }
        })
        .on_page_load(move |view, payload| {
            if view.label() != "main" {
                return;
            }
            if payload.event() == PageLoadEvent::Started {
                trace("splash content loading");
            }
            if payload.event() == PageLoadEvent::Finished {
                trace("splash loaded");
                // Require a mapped page, DOM layout and multiple animation frames.
                // Parent timeout handles a blank, hung or terminated WebProcess.
                if view.eval(r#"(() => {
                    document.querySelector('button').addEventListener('click', () => window.__TAURI_INTERNALS__.invoke('renderer_cancel'));
                    let frames = 0; const start = performance.now();
                    const frame = () => {
                        const logo = document.querySelector('.logo');
                        if (!logo || logo.getBoundingClientRect().width < 40) return;
                        logo.style.transform = `translate3d(${Math.sin(frames/4)*2}px,0,0)`;
                        if (++frames >= 12 && performance.now()-start >= 400) {
                            window.__TAURI_INTERNALS__.invoke('renderer_ready');
                        } else requestAnimationFrame(frame);
                    }; requestAnimationFrame(frame);
                })()"#).is_err() {
                    trace("could not schedule animation check");
                }
            }
        })
        .build(context);
    match result {
        Ok(app) => app.run_return(|_, _| {}),
        Err(_) => {
            trace("WebView initialization failed");
            1
        }
    }
}

fn finish(handle: &tauri::AppHandle, output: &Path, ok: bool) {
    handle.exit(if ok && std::fs::write(output, b"rendered-v1").is_ok() {
        0
    } else {
        1
    });
}

pub(super) fn outcome(status: std::process::ExitStatus, root: &Path) -> Outcome {
    if status.code() == Some(CANCELLED) {
        return Outcome::Cancelled;
    }
    if status.success()
        && std::fs::read(root.join("ready")).is_ok_and(|bytes| bytes == b"rendered-v1")
    {
        Outcome::Ready
    } else {
        Outcome::Failed
    }
}
