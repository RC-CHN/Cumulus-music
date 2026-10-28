//! Linux system-bus consumer proof: native Tauri/GTK + synthetic PCM/libmpv.
//! File commands exist only in this example and cannot enter production IPC.
#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod fixture;
#[cfg(target_os = "linux")]
#[path = "support/media_host.rs"]
mod proof;
#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "support/relay_fixture.rs"]
mod relay_fixture;
#[cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "support/settings_fixture.rs"]
mod settings_fixture;
#[cfg(target_os = "linux")]
fn main() {
    proof::run();
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("MPRIS proof requires Linux; it does not validate Windows SMTC.");
    std::process::exit(78);
}
