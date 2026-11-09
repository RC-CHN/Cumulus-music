//! One isolated production Tauri/Vue window with synthetic PC and CDN inputs.
#[cfg(target_os = "linux")]
#[path = "support/signed_window_read_ui.rs"]
mod proof;

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    proof::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("the synthetic signed window proof requires Linux");
    std::process::exit(2);
}
