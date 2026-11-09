//! Offline Windows scenarios exercising the same Win32 primitives as desktop.
#[cfg(all(test, windows))]
#[path = "../../../src/native_open/windows/effective.rs"]
mod effective;
#[cfg(all(test, windows))]
mod effective_cross_process;
#[cfg(windows)]
#[path = "../../../src/native_open/windows/identity.rs"]
mod identity;
#[cfg(windows)]
#[path = "../../../src/native_open/windows/security.rs"]
mod security;
#[cfg(windows)]
#[path = "../../../src/native_open/windows/stable.rs"]
mod stable;
#[cfg(windows)]
mod windows;
#[cfg(any(test, windows))]
#[path = "../../../src/native_open/windows/wire.rs"]
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
