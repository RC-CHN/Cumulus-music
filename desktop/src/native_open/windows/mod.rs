//! One profile owner, a private same-login handoff, and no renderer in the sender.
macro_rules! trace_handoff {
    ($stage:expr, $code:expr) => {{
        #[cfg(cumulus_windows_native_open_null_proof)]
        $crate::native_open::proof_audio::handoff_trace($stage, $code);
    }};
    ($stage:expr) => {{
        #[cfg(cumulus_windows_native_open_null_proof)]
        $crate::native_open::proof_audio::handoff_trace($stage, None);
    }};
}
pub(crate) use trace_handoff;

mod effective;
mod host;
mod identity;
mod profile;
mod profile_path;
mod security;
mod stable;
mod wire;

use super::{Pending, ProcessGuard, warm::Reply};
use identity::Identity;
use std::{
    io,
    path::{Path, PathBuf},
};

pub(crate) use host::{Relay, install};
pub(crate) use profile::Profile;

pub(crate) struct Claim {
    pub root: PathBuf,
    pub lease: ProcessGuard,
    pub listener: Option<host::Listener>,
}
impl Claim {
    pub fn matches_root(&self, root: &Path) -> io::Result<bool> {
        Ok(self
            .lease
            .windows_profile
            .as_ref()
            .is_some_and(|profile| profile.same_directory(root)))
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

pub(crate) fn profile_root(config: &tauri::Config) -> io::Result<PathBuf> {
    match std::env::var_os("CUMULUS_DATA_DIR") {
        Some(path) => Ok(PathBuf::from(path)),
        None => Ok(profile::roaming_app_data()?.join(&config.identifier)),
    }
}

fn claim(config: &tauri::Config, pending: &Pending) -> io::Result<Option<Claim>> {
    let root = profile_root(config)?;
    // Unsupported roots use the existing cold-only startup. No ordinary
    // filesystem resolution precedes the warm gate.
    let can_relay = std::env::var_os("CUMULUS_SESSION_DIR").is_none();
    let Ok(identity) = Identity::current() else {
        trace_handoff!("claim.identity_unavailable");
        return Ok(None);
    };
    let profile = match Profile::open(&root, true) {
        Ok(Some(profile)) => profile,
        Ok(None) => {
            trace_handoff!("claim.profile_unsupported");
            return Ok(None);
        }
        Err(_error) => {
            trace_handoff!("claim.profile_open_failed", _error.raw_os_error());
            return Ok(None);
        }
    };
    let (id, volume) = profile.identity();
    let address = identity.product_address(id, volume);
    // A suspicious lock in a verified directory is a claim error, never an
    // instruction to reopen that path through the legacy cold guard.
    match ProcessGuard::from_file(profile.lock_file()?, true) {
        Ok(mut lease) => {
            trace_handoff!("owner.profile_claimed");
            let listener = if can_relay {
                match host::Listener::bind(&address, &identity) {
                    Ok(listener) => {
                        trace_handoff!("owner.pipe_bound");
                        Some(listener)
                    }
                    Err(_error) => {
                        trace_handoff!("owner.pipe_bind_failed", _error.raw_os_error());
                        eprintln!(
                            "Cumulus native file handoff is unavailable; cold startup continues."
                        );
                        None
                    }
                }
            } else {
                None
            };
            let root = profile.path().to_owned();
            lease.windows_profile = Some(profile);
            Ok(Some(Claim {
                root,
                lease,
                listener,
            }))
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            if !can_relay || !pending.requested() {
                trace_handoff!("sender.duplicate_not_forwarded");
                Reply::Unavailable.finish();
            }
            trace_handoff!("sender.profile_owner_exists");
            host::forward(&address, &identity, &pending.take().unwrap_or_default()).finish();
        }
        Err(error) => Err(error),
    }
}
