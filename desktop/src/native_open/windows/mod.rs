//! One profile owner, a private same-login handoff, and no renderer in the sender.
mod host;
mod identity;
mod security;
mod stable;
mod wire;

use super::{Pending, ProcessGuard, warm::Reply};
use identity::Identity;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub(crate) use host::{Relay, install};

pub(crate) struct Claim {
    pub root: PathBuf,
    pub lease: ProcessGuard,
    pub listener: Option<host::Listener>,
    identity: Identity,
    address: String,
}
impl Claim {
    pub fn matches_root(&self, root: &Path) -> io::Result<bool> {
        let canonical = root.canonicalize()?;
        Ok(canonical == self.root && self.identity.product_address(&canonical)? == self.address)
    }
}

pub(crate) fn claim_or_exit(config: &tauri::Config, pending: &Pending) -> Option<Claim> {
    match claim(config, pending) {
        Ok(claim) => claim,
        Err(_) => {
            eprintln!("Cumulus could not claim its application data directory.");
            std::process::exit(2);
        }
    }
}

fn claim(config: &tauri::Config, pending: &Pending) -> io::Result<Option<Claim>> {
    let root = std::env::var_os("CUMULUS_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|data| data.join(&config.identifier)))
        .ok_or_else(|| io::Error::other("application data directory unavailable"))?;
    fs::create_dir_all(&root)?;
    let root = root.canonicalize()?;
    // Network/unsupported profile roots and unavailable Win32 identity keep
    // the existing cold-only process guard; no unverified endpoint is created.
    let Ok(identity) = Identity::current() else {
        return Ok(None);
    };
    let Ok(address) = identity.product_address(&root) else {
        return Ok(None);
    };
    let can_relay = std::env::var_os("CUMULUS_SESSION_DIR").is_none();
    match ProcessGuard::acquire(&root, true) {
        Ok(lease) => {
            let listener = if can_relay {
                match host::Listener::bind(&address, &identity) {
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
                identity,
                address,
            }))
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            if !can_relay || !pending.requested() {
                Reply::Unavailable.finish();
            }
            host::forward(&address, &identity, &pending.take().unwrap_or_default()).finish();
        }
        Err(error) => Err(error),
    }
}
