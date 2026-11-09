//! Stopped PC profile preparation for the native same-window read proof.
#[cfg(target_os = "linux")]
// This host binary uses only the export subset; the acceptance driver shares
// the in-process profile/context getters from the same support module.
#[allow(
    dead_code,
    reason = "host exporter uses a subset of shared native proof support"
)]
#[path = "support/signed_window_read_profile.rs"]
mod profile;

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    use profile::{IdentityMode, export::export_stopped};
    use std::{path::Path, process::ExitCode};

    let args: Vec<_> = std::env::args_os().skip(1).take(7).collect();
    let usage = "usage: signed_window_read_prepare --prepare SOURCE PRIVATE_UID_FILE INTENT_FILE NEW_PREPARED_DIR [--derive-if-missing]";
    let [flag, source, uid, intent, output, optional @ ..] = args.as_slice() else {
        eprintln!("{usage}");
        return ExitCode::FAILURE;
    };
    let identity = match optional {
        [] if flag == "--prepare" => IdentityMode::RequireSaved,
        [mode] if flag == "--prepare" && mode == "--derive-if-missing" => {
            IdentityMode::DeriveIfMissing
        }
        _ => {
            eprintln!("{usage}");
            return ExitCode::FAILURE;
        }
    };
    match export_stopped(
        Path::new(source),
        Path::new(uid),
        Path::new(intent),
        Path::new(output),
        identity,
    ) {
        Ok(derived) => {
            println!("{{\"ok\":true,\"identityWasDerived\":{derived}}}");
            ExitCode::SUCCESS
        }
        Err(_) => {
            eprintln!(
                "Signed proof preparation refused; check the stopped PC source and private input paths."
            );
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("The signed read proof preparation helper requires Linux.");
    std::process::ExitCode::FAILURE
}
