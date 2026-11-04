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
        let dmabuf_override = std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some();
        for &(key, value) in linux_defaults(&mode, wayland, nvidia, dmabuf_override) {
            if std::env::var_os(key).is_none() {
                // SAFETY: entry-point-only initialization, before GUI and workers.
                unsafe { std::env::set_var(key, value) };
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_defaults(
    mode: &str,
    wayland: bool,
    nvidia: bool,
    dmabuf_override: bool,
) -> &'static [(&'static str, &'static str)] {
    const MESA: (&str, &str) = ("LIBGL_ALWAYS_SOFTWARE", "1");
    const ENABLE_TRANSPORT: (&str, &str) = ("WEBKIT_FORCE_DMABUF_RENDERER", "1");
    const FORCE_SHM: (&str, &str) = ("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1");
    match mode {
        // Keep WebKit compositing. Forced shared memory resolves stale layers
        // on X11 but can stall frames on native NVIDIA Wayland.
        "software" if !wayland && !dmabuf_override => &[MESA, ENABLE_TRANSPORT, FORCE_SHM],
        "software" => &[MESA],
        "gpu-shm" => &[ENABLE_TRANSPORT, FORCE_SHM],
        "gpu" => &[ENABLE_TRANSPORT],
        "auto" if nvidia && wayland => &[ENABLE_TRANSPORT, ("__NV_DISABLE_EXPLICIT_SYNC", "1")],
        "auto" if nvidia => &[ENABLE_TRANSPORT, FORCE_SHM],
        _ => &[],
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::linux_defaults;

    #[test]
    fn software_renderer_retains_the_backend_specific_transport() {
        let mesa = ("LIBGL_ALWAYS_SOFTWARE", "1");
        assert_eq!(linux_defaults("software", true, true, false), &[mesa]);
        assert_eq!(linux_defaults("software", false, false, true), &[mesa]);
        assert_eq!(
            linux_defaults("software", false, false, false),
            &[
                mesa,
                ("WEBKIT_FORCE_DMABUF_RENDERER", "1"),
                ("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1")
            ]
        );
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
