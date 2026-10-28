//! Real Windows SMTC host and separate OS-session consumer, synthetic PCM only.
#[cfg(target_os = "windows")]
#[path = "support/smtc_consumer.rs"]
mod consumer;
#[cfg(target_os = "windows")]
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod fixture;
#[cfg(target_os = "windows")]
#[path = "support/media_host.rs"]
mod proof;
#[cfg(target_os = "windows")]
#[allow(dead_code)]
#[path = "support/relay_fixture.rs"]
mod relay_fixture;
#[cfg(target_os = "windows")]
#[allow(dead_code)]
#[path = "support/settings_fixture.rs"]
mod settings_fixture;
#[cfg(target_os = "windows")]
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--consumer") {
        consumer::run(&args[2], &args[3]).unwrap();
    } else {
        proof::run();
    }
}
#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("SMTC proof requires Windows; Linux checks are not runtime validation.");
    std::process::exit(78);
}
