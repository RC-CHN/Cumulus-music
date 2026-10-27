//! WebView rendering belongs to the desktop host, not the music core.
//!
//! Debian/Ubuntu WebKitGTK disables DMA-BUF for NVIDIA. On Wayland the driver
//! explicit-sync compatibility switch avoids the reproduced missing-acquire-point
//! error while retaining GPU buffer transport. Respect explicit user overrides.

/// Configure process-local renderer compatibility before starting the runtime.
///
/// # Safety
/// Call only on the initial thread BEFORE GTK/Tauri or any worker starts;
/// Linux environment mutations must not race with foreign-library getenv calls.
pub unsafe fn prepare_process() {
    #[cfg(target_os = "windows")]
    if effective_mode() == "software" {
        let mut args = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").unwrap_or_default();
        if !args.split_whitespace().any(|arg| arg == "--disable-gpu") {
            args.push_str(" --disable-gpu");
            // SAFETY: entry-point-only, before WebView2 or other threads start.
            unsafe { std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", args) };
        }
    }
    #[cfg(target_os = "linux")]
    {
        let mode = effective_mode();
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
            && std::env::var("GDK_BACKEND").as_deref() != Ok("x11");
        let nvidia = std::fs::read_dir("/sys/class/drm")
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("renderD"))
            .any(|entry| {
                std::fs::read_link(entry.path().join("device/driver"))
                    .ok()
                    .and_then(|p| p.file_name().map(|s| s == "nvidia"))
                    .unwrap_or(false)
            });
        let defaults: &[(&str, &str)] = match mode.as_str() {
            "software" => &[("WEBKIT_DISABLE_COMPOSITING_MODE", "1")],
            "gpu-shm" => &[
                ("WEBKIT_FORCE_DMABUF_RENDERER", "1"),
                ("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1"),
            ],
            "gpu" => &[("WEBKIT_FORCE_DMABUF_RENDERER", "1")],
            "auto" if nvidia && wayland => &[
                ("WEBKIT_FORCE_DMABUF_RENDERER", "1"),
                ("__NV_DISABLE_EXPLICIT_SYNC", "1"),
            ],
            "auto" if nvidia => &[
                ("WEBKIT_FORCE_DMABUF_RENDERER", "1"),
                ("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1"),
            ],
            _ => &[],
        };
        for &(key, value) in defaults {
            if std::env::var_os(key).is_none() {
                // SAFETY: entry-point-only initialization, before GUI and workers.
                unsafe { std::env::set_var(key, value) };
            }
        }
    }
}

pub(super) fn effective_mode() -> String {
    std::env::var("CUMULUS_RENDERER_EFFECTIVE")
        .or_else(|_| std::env::var("CUMULUS_RENDERER"))
        .unwrap_or_else(|_| "auto".into())
}

pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("graphics")
        .on_webview_ready(|webview| {
            #[cfg(target_os = "linux")]
            if webview.label() == "main" {
                let _ = webview.with_webview(|view| linux::configure(&view.inner()));
            }
            #[cfg(not(target_os = "linux"))]
            let _ = webview; // WebView2 enables hardware acceleration by default.
        })
        .build()
}

mod cache;
#[cfg(target_os = "windows")]
mod job;
#[cfg(target_os = "linux")]
mod linux;
mod policy;
mod probe;
mod startup;
pub use startup::bootstrap;
