//! Explicit Windows x64 registry/NDIS probe. Output is availability only.

use std::process::ExitCode;

#[cfg(any(test, all(target_os = "windows", target_arch = "x86_64")))]
#[path = "../src/identity/windows/mac.rs"]
mod mac;

fn main() -> ExitCode {
    if !std::env::args_os()
        .skip(1)
        .eq([std::ffi::OsStr::new("--collect")])
    {
        eprintln!("Usage: windows_mac_probe --collect (Windows x64; status only)");
        return ExitCode::from(2);
    }
    collect()
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn collect() -> ExitCode {
    use mac::{Fallback, Inconclusive, Observation};
    match mac::collect_registry_mac() {
        Observation::RegistryNdis(address) => {
            println!(
                "status=registry_ndis_available formatted_bytes={}",
                address.official_ascii().len()
            );
            ExitCode::SUCCESS
        }
        Observation::FallbackRequired(reason) => {
            let label = match reason {
                Fallback::NoRegistryCandidate => "no_registry_candidate",
                Fallback::RejectedByOuterFilter => "outer_filter_rejected",
            };
            println!("status=fallback_required reason={label}");
            ExitCode::from(3)
        }
        Observation::Inconclusive(reason) => {
            let label = match reason {
                Inconclusive::EnumerationLimit => "enumeration_limit",
                Inconclusive::RegistryKey => "registry_key",
                Inconclusive::Description => "description",
                Inconclusive::DeviceName => "device_name",
                Inconclusive::DriverReply => "driver_reply",
            };
            println!("status=inconclusive reason={label}");
            ExitCode::from(4)
        }
    }
}

#[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
fn collect() -> ExitCode {
    println!("status=windows_x64_required");
    ExitCode::from(2)
}
