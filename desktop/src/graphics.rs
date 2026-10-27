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
    #[cfg(target_os = "linux")]
    {
        let mode = std::env::var("CUMULUS_RENDERER").unwrap_or_else(|_| "auto".into());
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

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::{CStr, c_char, c_int, c_void};
    use webkit2gtk::{SettingsExt, WebViewExt, glib::translate::ToGlibPtr};

    // These public WebKitGTK APIs are available since 2.42 but absent from the
    // gtk-rs 0.18 bindings used by Tauri. Keep the untyped boundary here only.
    unsafe extern "C" {
        fn webkit_settings_get_all_features() -> *mut c_void;
        fn webkit_feature_list_get_length(list: *mut c_void) -> usize;
        fn webkit_feature_list_get(list: *mut c_void, index: usize) -> *mut c_void;
        fn webkit_feature_get_identifier(feature: *mut c_void) -> *const c_char;
        fn webkit_settings_set_feature_enabled(
            settings: *mut webkit2gtk::ffi::WebKitSettings,
            feature: *mut c_void,
            enabled: c_int,
        );
        fn webkit_feature_list_unref(list: *mut c_void);
    }
    pub fn configure(view: &webkit2gtk::WebView) {
        let Some(settings) = view.settings() else {
            return;
        };
        settings.set_enable_smooth_scrolling(true);
        if std::env::var("CUMULUS_RENDERER").as_deref() == Ok("software") {
            settings
                .set_hardware_acceleration_policy(webkit2gtk::HardwareAccelerationPolicy::Never);
        } else {
            settings
                .set_hardware_acceleration_policy(webkit2gtk::HardwareAccelerationPolicy::Always);
        }
        // SAFETY: runs on the GTK main thread; settings stays owned throughout;
        // each borrowed feature/string is only used while the owning list lives.
        unsafe {
            let list = webkit_settings_get_all_features();
            if list.is_null() {
                return;
            }
            for index in 0..webkit_feature_list_get_length(list) {
                let feature = webkit_feature_list_get(list, index);
                if feature.is_null() {
                    continue;
                }
                let identifier = webkit_feature_get_identifier(feature);
                if !identifier.is_null()
                    && CStr::from_ptr(identifier).to_bytes()
                        == b"PreferPageRenderingUpdatesNear60FPS"
                {
                    webkit_settings_set_feature_enabled(settings.to_glib_none().0, feature, 0);
                }
            }
            webkit_feature_list_unref(list);
        }
    }
}
