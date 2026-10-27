use cumulus_core::{Error, Result, protocol::http::ClientProfile};
use std::path::Path;

/// Native launch configuration only; no fingerprint or path is accepted via IPC.
pub fn profile(root: &Path) -> Result<ClientProfile> {
    match std::env::var_os("CUMULUS_DEVICE_IDENTITY") {
        None => Ok(ClientProfile::default()),
        Some(value) if value == "local" => local(root, ClientProfile::default()),
        Some(value) if value == "windows-local" => local(
            root,
            ClientProfile::windows_compatibility(
                cumulus_core::identity::WINDOWS_COMPATIBILITY_DESCRIPTION,
            )?,
        ),
        Some(_) => Err(Error::InvalidInput(
            "CUMULUS_DEVICE_IDENTITY must be local, windows-local or unset",
        )),
    }
}
#[cfg(target_os = "linux")]
fn local(root: &Path, base: ClientProfile) -> Result<ClientProfile> {
    use cumulus_core::{
        identity::{IdentityService, collect_linux_identity},
        storage::StateStore,
    };
    let facts = collect_linux_identity(Path::new("/sys"))?;
    IdentityService::new(std::sync::Arc::new(StateStore::open(root)?))
        .resolve(&facts)?
        .profile(base)
}
#[cfg(not(target_os = "linux"))]
fn local(_root: &Path, _base: ClientProfile) -> Result<ClientProfile> {
    Err(Error::State(
        "local hardware collector is not implemented on this platform",
    ))
}
