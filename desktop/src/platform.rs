//! Process-wide media prerequisites belong to the desktop host, before its
//! player workers start, rather than to arbitrary Rust core callers.
pub fn initialize_numeric_locale() -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        // GTK initializes the process locale from the user's environment.
        // libmpv checks the global LC_NUMERIC name in mpv_create (uselocale
        // cannot satisfy it). Keep language/encoding/date categories intact.
        // SAFETY: called once during Tauri plugin startup, after GTK runtime
        // initialization and before application/player initialization. The
        // input is static and no locale pointer escapes this call.
        let result = unsafe { libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr()) };
        if result.is_null() {
            return Err(std::io::Error::other(
                "cannot initialize media numeric locale",
            ));
        }
    }
    Ok(())
}

/// True when the native main window owns the foreground used by the queue cue.
/// Browser document focus is checked independently in the renderer.
#[doc(hidden)]
pub fn queue_tip_window_foreground<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> tauri::Result<bool> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let own = window.hwnd()?.0;
        // Tao's Windows focused state requires the top-level HWND itself to
        // own keyboard focus. WebView2 can put it on a child of that HWND
        // while the top-level window still legitimately owns the foreground.
        // SAFETY: read-only Win32 query; no handles or ownership are changed.
        let foreground = unsafe { GetForegroundWindow() };
        Ok(!own.is_null() && foreground == own)
    }
    #[cfg(not(windows))]
    window.is_focused()
}
