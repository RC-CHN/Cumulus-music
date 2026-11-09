//! Explicit offline diagnostic. Never read account state, print serial data,
//! or pass a hardware result to Cumulus.
#[cfg(any(all(windows, target_arch = "x86_64"), test))]
mod policy;

#[cfg(all(windows, target_arch = "x86_64"))]
#[path = "../../../../scripts/research/windows_identity_board_rules.rs"]
mod windows_identity_board_rules;

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows;

#[cfg(all(windows, target_arch = "x86_64"))]
fn main() -> std::process::ExitCode {
    use std::{process::ExitCode, sync::mpsc, thread, time::Duration};

    if std::env::args_os().len() != 1 {
        eprintln!("This local probe takes no arguments.");
        return ExitCode::FAILURE;
    }
    let (send, receive) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("board-wmi-mta".into())
        .spawn(move || {
            let _ = send.send(windows::run());
        });
    if worker.is_err() {
        eprintln!("board_probe=worker_unavailable");
        return ExitCode::FAILURE;
    }
    match receive.recv_timeout(Duration::from_secs(12)) {
        Ok(Ok(report)) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Ok(Err(error)) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            eprintln!("board_probe=deadline");
            ExitCode::FAILURE
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            eprintln!("board_probe=worker_failed");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() -> std::process::ExitCode {
    eprintln!("The native WMI board probe requires Windows x86_64.");
    std::process::ExitCode::FAILURE
}
