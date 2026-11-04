//! Explicit passive diagnostic; it never changes the interface table.

#[cfg(all(windows, target_arch = "x86_64"))]
fn main() {
    use cumulus_windows_address_watch::{AddressWatcher, Close, NativeError};

    let Ok(first) = AddressWatcher::start() else {
        eprintln!("The local IPv4 notification could not be registered.");
        std::process::exit(1);
    };
    let exclusive = matches!(AddressWatcher::start(), Err(NativeError::Active));
    let first_closed = first.close() == Close::Completed;
    let reopened = first_closed
        && AddressWatcher::start().is_ok_and(|second| second.close() == Close::Completed);
    println!(
        "{{\"nativeWindowsIpv4Watch\":true,\"exclusive\":{exclusive},\"terminalClose\":{first_closed},\"reopenedAndClosed\":{reopened},\"addressDataRead\":false,\"playbackEnabled\":false}}"
    );
    if !(exclusive && first_closed && reopened) {
        std::process::exit(1);
    }
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() {
    eprintln!("This passive diagnostic requires Windows x86_64.");
    std::process::exit(2);
}
