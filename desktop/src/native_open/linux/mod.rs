//! Profile ownership and lifecycle of the private Linux open-file relay.
mod host;
mod wire;

use super::{Pending, ProcessGuard};
use std::{
    fs, io,
    os::unix::{fs::MetadataExt, net::UnixListener},
    path::PathBuf,
};
use wire::{Reply, address, effective_uid, forward};

pub(crate) use host::{Relay, install};

pub(crate) fn claim_or_exit(config: &tauri::Config, pending: &Pending) -> Option<Claim> {
    match claim(config, pending) {
        Ok(claim) => claim,
        Err(_) => {
            eprintln!("Cumulus could not claim its application data directory.");
            std::process::exit(2);
        }
    }
}

pub(crate) struct Claim {
    pub root: PathBuf,
    pub lease: ProcessGuard,
    pub listener: Option<UnixListener>,
}

fn claim(config: &tauri::Config, pending: &Pending) -> io::Result<Option<Claim>> {
    let override_root = std::env::var_os("CUMULUS_DATA_DIR").map(PathBuf::from);
    // Match the pinned Tauri 2.11.5 PathResolver::app_data_dir rule.
    // The production setup independently checks the same canonical root.
    let root = override_root
        .or_else(|| dirs::data_dir().map(|data| data.join(&config.identifier)))
        .ok_or_else(|| io::Error::other("application data directory unavailable"))?;
    fs::create_dir_all(&root)?;
    let root = root.canonicalize()?;
    let metadata = root.metadata()?;
    // Only app data controlled by this effective user participates. An
    // explicitly shared directory retains its previous cold-only behavior.
    if metadata.uid() != effective_uid() || metadata.mode() & 0o022 != 0 {
        return Ok(None);
    }
    let can_relay = std::env::var_os("CUMULUS_SESSION_DIR").is_none();
    let address = address(&root)?;
    match ProcessGuard::acquire(&root, true) {
        Ok(lease) => {
            let listener = if can_relay {
                match UnixListener::bind_addr(&address) {
                    Ok(listener) => Some(listener),
                    Err(_) => {
                        eprintln!(
                            "Cumulus native file handoff is unavailable; cold startup continues."
                        );
                        None
                    }
                }
            } else {
                None
            };
            Ok(Some(Claim {
                root,
                lease,
                listener,
            }))
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            if !pending.requested() {
                eprintln!("Cumulus is already running for this app profile.");
                std::process::exit(2);
            }
            if !can_relay {
                Reply::Unavailable.finish();
            }
            let paths = pending.take().unwrap_or_default();
            forward(&address, &paths).finish();
        }
        Err(error) => Err(error),
    }
}
