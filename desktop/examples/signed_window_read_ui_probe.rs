//! One isolated production Tauri/Vue window with synthetic PC and CDN inputs.
#[cfg(target_os = "linux")]
#[path = "support/signed_window_read_ui.rs"]
mod proof;

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "signed-window-read-proof")]
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--live")) {
        // A real proof emits only the sanitized report, including on failure.
        std::process::exit(proof::live::run());
    }
    proof::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("the synthetic signed window proof requires Linux");
    std::process::exit(2);
}
