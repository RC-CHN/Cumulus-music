//! Explicit real-account download pipeline proof. Three isolated outputs,
//! current server authorizations, no remote account mutations or payments.
use cumulus_core::{
    Error, Result,
    account::{AccountApi, AccountSession, credentials::Credentials},
    download::{
        offline::OfflineLibrary,
        service::{
            DownloadDestination, DownloadOptions, DownloadRunOptions, DownloadScheduler,
            DownloadService, DownloadStatus, OutputContainer,
        },
    },
    media::ncm::{HeaderPolicy, NcmReader},
    membership::Quality,
    protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

async fn session(profile: &Path) -> Result<Arc<AccountSession>> {
    let store = Arc::new(StateStore::open(profile)?);
    let credentials = Arc::new(Credentials::open(store.clone(), ApiOrigin::Pc)?);
    let api = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
    let session = Arc::new(AccountSession::new(api, Some(store)));
    session.refresh().await?;
    Ok(session)
}
fn report(path: &Path, value: &Value) -> Result<()> {
    let mut opts = OpenOptions::new();
    opts.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
    f.sync_all()?;
    Ok(())
}
async fn run() -> Result<Value> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "usage: download_roundtrip EXISTING_PC_PROFILE NEW_OUTPUT_DIRECTORY",
        ));
    }
    let profile = PathBuf::from(&args[0]);
    if !profile.join("core.sqlite3").is_file() {
        return Err(Error::InvalidInput("existing private profile required"));
    }
    let output = PathBuf::from(&args[1]);
    let builder = cumulus_core::storage::private_directory_builder();
    builder.create(&output)?;
    let account = session(&profile).await?;
    let token = account.token()?;
    let ids = account
        .run_for(&token, |api, uid| async move {
            AccountApi::new(api).liked_ids(&json!(uid)).await
        })
        .await?;
    if ids.len() < 2 {
        return Err(Error::State("two songs required for probe"));
    }
    let service = DownloadService::new(account)?;
    let mut task_ids = Vec::new();
    for index in 0..3 {
        let container = if index == 2 {
            OutputContainer::Ncm
        } else {
            OutputContainer::Audio
        };
        let task = service.create(
            &json!(ids[index % 2]),
            DownloadDestination::Path {
                path: output.join(format!("track-{index}.media")),
            },
            DownloadOptions {
                quality: Quality::Lossless,
                lyrics: index != 2,
                container,
                ..Default::default()
            },
        )?;
        task_ids.push(task.id);
    }
    report(&output.join("private-task-ids.json"), &json!(task_ids))?;
    let paused = service
        .run(
            &task_ids[0],
            DownloadRunOptions {
                byte_budget: Some(65537),
                ..Default::default()
            },
        )
        .await?;
    if paused.status != DownloadStatus::Paused || paused.bytes != 65537 {
        return Err(Error::State("probe did not pause at expected offset"));
    }
    drop(service);
    let account = session(&profile).await?;
    let token = account.token()?;
    let service = DownloadService::new(account.clone())?;
    service.enqueue(&task_ids[0])?;
    let results = DownloadScheduler::new(service.clone(), 3)?
        .run_selected(&task_ids)
        .await?;
    if results.iter().any(|v| v.error.is_some()) {
        return Err(Error::State(
            "download queue has an incomplete task; inspect its private journal",
        ));
    }
    let offline = OfflineLibrary::new(account)?;
    let mut cases = Vec::new();
    for (index, id) in task_ids.iter().enumerate() {
        let task = service.get(id)?;
        if task.status != DownloadStatus::Complete || task.cleanup_pending || task.work.exists() {
            return Err(Error::State("download pipeline did not finish cleanly"));
        }
        let path = task.target.as_ref().ok_or(Error::State("missing output"))?;
        let checked = offline.check(&token, path, false).await?;
        if !checked.access.can_play || !checked.integrity_verified {
            return Err(Error::State(
                "offline access proof failed; inspect account entitlement",
            ));
        }
        if task.encrypted {
            let mut reader = NcmReader::open(path, HeaderPolicy::Strict)?;
            let mut opts = OpenOptions::new();
            opts.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut target = opts.open(output.join("ncm-decoded.media"))?;
            std::io::copy(&mut reader, &mut target)?;
            target.sync_all()?;
        }
        let resource = task
            .resource
            .as_ref()
            .ok_or(Error::State("missing resource"))?;
        cases.push(json!({"case":index,"ok":true,"encrypted":task.encrypted,"level":resource["level"],"format":resource["type"],"authorization_count":task.authorization_count,"raw_integrity":task.raw_integrity,"output_integrity":task.output_integrity,"lyrics":task.lyrics_status,"offline_access":checked.access.can_play,"offline_integrity":checked.integrity_verified,"cleanup_complete":!task.cleanup_pending}));
    }
    Ok(
        json!({"schema":1,"implementation":"rust-core","stage":"full_track_download_pipeline","remote_mutations":0,"synthetic_media":false,"paused_bytes":paused.bytes,"reopened":true,"scheduler_concurrency":3,"cases":cases}),
    )
}
#[tokio::main]
async fn main() {
    match run().await {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).expect("report")),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1)
        }
    }
}
