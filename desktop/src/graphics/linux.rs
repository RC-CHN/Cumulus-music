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
    if super::effective_mode() == "software" {
        settings.set_hardware_acceleration_policy(webkit2gtk::HardwareAccelerationPolicy::Never);
    } else {
        settings.set_hardware_acceleration_policy(webkit2gtk::HardwareAccelerationPolicy::Always);
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
                && CStr::from_ptr(identifier).to_bytes() == b"PreferPageRenderingUpdatesNear60FPS"
            {
                webkit_settings_set_feature_enabled(settings.to_glib_none().0, feature, 0);
            }
        }
        webkit_feature_list_unref(list);
    }
}
