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
