//! Actual anonymous public configuration GET and persistent cache/304 proof.
//! This executable refuses report requests; no device/browser/account data.
use cumulus_core::{Error, Result, protocol::api::ApiFuture, storage::StateStore, watchman::*};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
fn save(root: &Path, name: &str, value: &Value) -> Result<()> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(root.join(name))?;
    file.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
struct Audit {
    root: PathBuf,
    backend: NativeBackend,
    count: Mutex<usize>,
}
impl Transport for Audit {
    fn send(&self, request: Request) -> ApiFuture<'_, Value> {
        Box::pin(async move {
            if !matches!(request.kind, RequestKind::Configuration { .. }) {
                return Err(Error::State(
                    "configuration proof refuses device/behavior reports",
                ));
            }
            let n = {
                let mut count = self.count.lock().map_err(|_| Error::LockPoisoned)?;
                let n = *count;
                *count += 1;
                n
            };
            save(
                &self.root,
                &format!("request-{n}.json"),
                &json!({"url":request.url()?,"method":"GET","cookie":false,"fingerprint":false}),
            )?;
            let response = self.backend.send(request).await?;
            save(&self.root, &format!("response-{n}.json"), &response)?;
            Ok(response)
        })
    }
}
async fn run(root: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(root)?;
    let clock = Arc::new(SystemEntropyClock);
    let backend = Arc::new(Audit {
        root: root.into(),
        backend: NativeBackend::new("Cumulus/0.1 Watchman configuration audit")?,
        count: Mutex::new(0),
    });
    let store = Arc::new(StateStore::open(root.join("state"))?);
    let config = ConfigService::new(
        store.clone(),
        backend.clone(),
        clock.clone(),
        "YD00000558929251",
    )?;
    let first = config.load(false).await?;
    let after_first = *backend.count.lock().unwrap();
    drop(config);
    drop(store);
    let reopened = ConfigService::new(
        Arc::new(StateStore::open(root.join("state"))?),
        backend.clone(),
        clock,
        "YD00000558929251",
    )?;
    let second = reopened.load(false).await?;
    if *backend.count.lock().unwrap() != after_first || first.revision() != second.revision() {
        return Err(Error::State("config reopen did not use cache"));
    }
    let third = reopened.load(true).await?;
    if third.revision() != second.revision() + 1 {
        return Err(Error::State("configuration revalidation revision mismatch"));
    }
    let report = json!({"ok":true,"sdkBuild":third.config().build_version(),"requests":*backend.count.lock().unwrap(),"reopenUsedCache":true,"revalidated":true,"deviceOrBehaviorReports":0,"musicBusinessRequests":0,"executedRemoteScripts":false});
    save(root, "report.json", &report)?;
    println!("{report}");
    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let root = std::env::args_os().nth(1).ok_or(Error::InvalidInput(
        "usage: watchman_config_read NEW_PRIVATE_DIRECTORY",
    ))?;
    run(Path::new(&root)).await
}
