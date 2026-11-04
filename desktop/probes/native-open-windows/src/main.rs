//! Offline Windows transport research; not linked by the desktop application.
#[cfg(windows)]
mod identity;
#[cfg(windows)]
mod stable;
#[cfg(windows)]
mod windows;
#[cfg(any(test, windows))]
mod wire;

#[cfg(windows)]
fn main() {
    if let Err(error) = windows::run() {
        // Report only Win32 status and broad error kind, never a path, SID or
        // token. All normal successful output is fixed synthetic evidence.
        eprintln!(
            "native open lab failed: {:?} ({:?})",
            error.kind(),
            error.raw_os_error()
        );
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("native open transport requires Windows x64");
    std::process::exit(2);
}
