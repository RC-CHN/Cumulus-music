//! Native device-record utility. Offline; diagnostics never print identifiers.
use cumulus_core::{Error, Result, identity::*, storage::StateStore};
use serde_json::json;
use std::{fs, io::Write, path::Path, sync::Arc};
fn output_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.as_slice() {
        [action, path] if action == "inspect" => {
            let report = inspect_container(&read_container(Path::new(path))?)?;
            println!("{}", json!({"records":report,"networkRequests":0}));
        }
        [action, state] if action == "collect" => {
            if !cfg!(target_os = "linux") {
                return Err(Error::State(
                    "local hardware collector is not implemented on this platform",
                ));
            }
            let facts = collect_linux_identity(Path::new("/sys"))?;
            let identity =
                IdentityService::new(Arc::new(StateStore::open(state)?)).resolve(&facts)?;
            println!(
                "{}",
                json!({"summary":identity.summary(),"persistentIdValid":valid_persistent_id(identity.persistent_id()),"networkRequests":0})
            );
        }
        [action, state, path] if action == "import" => {
            IdentityService::new(Arc::new(StateStore::open(state)?))
                .import_primary(&read_container(Path::new(path))?)?;
            println!(
                "{}",
                json!({"imported":true,"existingIdentityReplaced":false,"networkRequests":0})
            );
        }
        [action, state, path] if action == "export" => {
            let bytes =
                IdentityService::new(Arc::new(StateStore::open(state)?)).export_primary()?;
            output_new(Path::new(path), &bytes)?;
            println!(
                "{}",
                json!({"records":inspect_container(&bytes)?,"networkRequests":0})
            );
        }
        _ => {
            return Err(Error::InvalidInput(
                "usage: device_identity inspect FILE | collect STATE | import STATE FILE | export STATE NEW_FILE",
            ));
        }
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("device identity: {e}");
        std::process::exit(1);
    }
}
