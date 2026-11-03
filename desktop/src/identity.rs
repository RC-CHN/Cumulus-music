use cumulus_core::{Error, Result, protocol::http::ClientProfile};
use std::{ffi::OsStr, path::Path};

/// Native launch configuration only; no fingerprint or path is accepted via IPC.
pub fn profile(root: &Path) -> Result<ClientProfile> {
    IdentityMode::from_override(std::env::var_os("CUMULUS_DEVICE_IDENTITY").as_deref())?
        .build(root, local)
}

/// Construction-only selection, separate from stored account and device IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdentityMode {
    Native,
    Local,
    WindowsAuto,
    WindowsLocal,
}

impl IdentityMode {
    fn from_override(value: Option<&OsStr>) -> Result<Self> {
        match value {
            None => Ok(Self::WindowsAuto),
            Some(value) if value == "native" => Ok(Self::Native),
            Some(value) if value == "local" => Ok(Self::Local),
            Some(value) if value == "windows-local" => Ok(Self::WindowsLocal),
            Some(_) => Err(Error::InvalidInput(
                "CUMULUS_DEVICE_IDENTITY must be native, local, windows-local or unset",
            )),
        }
    }

    fn build(
        self,
        root: &Path,
        collect_local: impl FnOnce(&Path, ClientProfile, bool) -> Result<ClientProfile>,
    ) -> Result<ClientProfile> {
        match self {
            Self::Native => Ok(ClientProfile::default()),
            Self::Local => collect_local(root, ClientProfile::default(), false),
            Self::WindowsAuto | Self::WindowsLocal => {
                let base = ClientProfile::windows_compatibility(
                    cumulus_core::identity::WINDOWS_COMPATIBILITY_DESCRIPTION,
                )?;
                // A Windows declaration is available on Windows, but its real
                // hardware collector is not implemented. Never run Linux sysfs
                // collection there or invent hardware/signature components.
                if cfg!(target_os = "windows") {
                    Ok(base)
                } else {
                    collect_local(root, base, self == Self::WindowsAuto)
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn local(root: &Path, base: ClientProfile, allow_missing_hardware: bool) -> Result<ClientProfile> {
    local_from_sysroot(root, Path::new("/sys"), base, allow_missing_hardware)
}

#[cfg(target_os = "linux")]
fn local_from_sysroot(
    root: &Path,
    sysroot: &Path,
    base: ClientProfile,
    allow_missing_hardware: bool,
) -> Result<ClientProfile> {
    use cumulus_core::{
        identity::{IdentityService, collect_linux_identity},
        storage::StateStore,
    };
    let facts = match collect_linux_identity(sysroot) {
        Ok(facts) => facts,
        Err(Error::State("hardware identity unavailable")) if allow_missing_hardware => {
            static NOTICE: std::sync::Once = std::sync::Once::new();
            NOTICE.call_once(|| {
                eprintln!(
                    "Cumulus: local hardware identity unavailable; Windows compatibility declarations active without clientSign"
                );
            });
            return Ok(base);
        }
        Err(error) => return Err(error),
    };
    // Store and identity errors must propagate; they are outside the collector's
    // narrowly defined absence fallback and must never rotate persisted IDs.
    IdentityService::new(std::sync::Arc::new(StateStore::open(root)?))
        .resolve(&facts)?
        .profile(base)
}

#[cfg(not(target_os = "linux"))]
fn local(
    _root: &Path,
    _base: ClientProfile,
    _allow_missing_hardware: bool,
) -> Result<ClientProfile> {
    Err(Error::State(
        "local hardware collector is not implemented on this platform",
    ))
}

#[cfg(test)]
#[path = "../tests/identity/device_profile.rs"]
mod tests;
