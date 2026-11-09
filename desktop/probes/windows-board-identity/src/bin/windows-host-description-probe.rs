//! Explicit native probe for WMI host-description inputs, with no private output.
#[cfg(any(all(windows, target_arch = "x86_64"), test))]
#[path = "../host_shape.rs"]
mod host_shape;

#[cfg(all(windows, target_arch = "x86_64"))]
#[path = "../host.rs"]
mod host;
#[cfg(all(windows, target_arch = "x86_64"))]
#[path = "../wmi.rs"]
mod wmi;

#[cfg(all(windows, target_arch = "x86_64"))]
fn main() -> std::process::ExitCode {
    use std::{process::ExitCode, sync::mpsc, thread, time::Duration};

    if std::env::args_os().len() != 1 {
        eprintln!("This local probe takes no arguments.");
        return ExitCode::FAILURE;
    }
    let (send, receive) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("host-wmi-mta".into())
        .spawn(move || {
            let _ = send.send(wmi::with_local_wmi(host::observe));
        });
    if worker.is_err() {
        eprintln!("host_probe=worker_unavailable");
        return ExitCode::FAILURE;
    }
    match receive.recv_timeout(Duration::from_secs(12)) {
        Ok(Ok(report)) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Ok(Err(error)) => {
            eprintln!("host_probe=startup_failed {error}");
            ExitCode::FAILURE
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            eprintln!("host_probe=deadline");
            ExitCode::FAILURE
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            eprintln!("host_probe=worker_failed");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() -> std::process::ExitCode {
    eprintln!("The native WMI host-description probe requires Windows x86_64.");
    std::process::ExitCode::FAILURE
}
