//! Explicit Windows x64 MAC observations. Output is availability only.

use std::process::ExitCode;

#[cfg(any(test, all(target_os = "windows", target_arch = "x86_64")))]
#[path = "../src/identity/windows/mac.rs"]
mod mac;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    match (args.next(), args.next()) {
        (Some(arg), None) if arg == "--collect" => collect(),
        (Some(arg), None) if arg == "--collect-fallback" => collect_fallback(),
        _ => {
            eprintln!(
                "Usage: windows_mac_probe --collect | --collect-fallback (Windows x64; status only)"
            );
            ExitCode::from(2)
        }
    }
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

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn collect_fallback() -> ExitCode {
    use mac::{FallbackError, FallbackObservation, FallbackSource};
    match mac::collect_fallback_mac() {
        Ok(FallbackObservation::Available(source, address)) => {
            let label = match source {
                FallbackSource::Ndis => "ndis",
                FallbackSource::AdapterRecord => "adapter_record",
            };
            println!(
                "status=fallback_available source={label} formatted_bytes={}",
                address.official_ascii().len()
            );
            ExitCode::SUCCESS
        }
        Ok(FallbackObservation::NoMatch) => {
            println!("status=fallback_unavailable reason=no_matching_adapter");
            ExitCode::from(3)
        }
        Ok(FallbackObservation::RejectedByOuterFilter) => {
            println!("status=fallback_unavailable reason=outer_filter_rejected");
            ExitCode::from(3)
        }
        Err(reason) => {
            let label = match reason {
                FallbackError::WinsockStartup => "winsock_startup",
                FallbackError::HostName => "host_name",
                FallbackError::HostIpv4 => "host_ipv4",
                FallbackError::AdapterList => "adapter_list",
                FallbackError::AdapterIp => "adapter_ip",
                FallbackError::NameConversion => "name_conversion",
                FallbackError::DeviceName => "device_name",
                FallbackError::DriverReply => "driver_reply",
                FallbackError::AddressLength => "address_length",
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

#[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
fn collect_fallback() -> ExitCode {
    collect()
}
