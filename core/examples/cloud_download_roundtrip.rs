//! Explicit read-only account proof: cloud original -> pause/reopen -> raw file,
//! offline record -> rename/move/delete. Only newly created local files change.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    cloud::CloudApi,
    download::{
        completed::{CompletedDownloads, FileAction},
        offline::OfflineLibrary,
        service::{DownloadDestination, DownloadRunOptions, DownloadService, DownloadStatus},
    },
    media::integrity::inspect_file,
    protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};
async fn account(profile: &Path, state: &Path) -> Result<Arc<AccountSession>> {
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let api = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
    let session = Arc::new(AccountSession::new(
        api,
        Some(Arc::new(StateStore::open(state)?)),
    ));
    session.refresh().await?;
    Ok(session)
}
async fn snapshot(session: &AccountSession) -> Result<cumulus_core::cloud::library::CloudSnapshot> {
    session
        .run_for(&session.token()?, |api, _| async move {
            CloudApi::new(api).snapshot().await
        })
        .await
}
async fn run(profile: &Path, root: &Path) -> Result<Value> {
    let session = account(profile, &root.join("state")).await?;
    let baseline = snapshot(&session).await?;
    let entry = baseline
        .items
        .iter()
        .filter(|entry| entry.file_size > 131071 && entry.file_size < 8 * 1024 * 1024)
        .filter(|entry| {
            Path::new(&entry.file_name)
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| {
                    matches!(
                        s.to_ascii_lowercase().as_str(),
                        "mp3" | "aac" | "m4a" | "flac"
                    )
                })
        })
        .min_by_key(|entry| entry.file_size)
        .ok_or(Error::State("no small account cloud audio sample"))?;
    let service = DownloadService::new(session.clone())?;
    let created = service
        .create_cloud(
            &json!(entry.song_id),
            DownloadDestination::Named {
                root: root.join("media"),
                path_type: 0,
                file_type: 1,
            },
            true,
        )
        .await?;
    let paused = service
        .run(
            &created.id,
            DownloadRunOptions {
                byte_budget: Some(131071),
                ..Default::default()
            },
        )
        .await?;
    if paused.status != DownloadStatus::Paused || paused.bytes != 131071 {
        return Err(Error::State("cloud pause checkpoint not reached"));
    }
    fs::write(
        root.join("paused.json"),
        serde_json::to_vec_pretty(
            &json!({"paused":true,"bytes":paused.bytes,"cloud_origin":paused.origin.is_cloud()}),
        )?,
    )?;
    drop(service);
    drop(session);
    let reopened = account(profile, &root.join("state")).await?;
    let service = DownloadService::new(reopened.clone())?;
    service.enqueue(&created.id)?;
    let completed = service
        .run(&created.id, DownloadRunOptions::default())
        .await?;
    if completed.status != DownloadStatus::Complete
        || completed.encrypted
        || !completed.origin.is_cloud()
    {
        return Err(Error::State("cloud original not completed"));
    }
    let path = completed
        .target
        .as_ref()
        .ok_or(Error::State("cloud output missing"))?;
    let integrity = inspect_file(path)?;
    let raw = completed
        .raw_integrity
        .as_ref()
        .ok_or(Error::State("raw cloud digest missing"))?;
    if integrity.bytes != entry.file_size
        || integrity.bytes != raw.bytes
        || integrity.sha256 != raw.sha256
    {
        return Err(Error::Media("cloud original bytes changed"));
    }
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_name,sample_rate,channels",
            "-of",
            "json",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .output()?;
    if !probe.status.success() {
        return Err(Error::Media("cloud original probe failed"));
    }
    let media: Value = serde_json::from_slice(&probe.stdout)?;
    let decoded = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(path)
        .args(["-f", "null", "-"])
        .stdin(Stdio::null())
        .output()?;
    if !decoded.status.success() {
        return Err(Error::Media("cloud original full decode failed"));
    }
    let offline = OfflineLibrary::new(reopened.clone())?;
    let check = offline.check(&reopened.token()?, path, true).await?;
    if !check.access.can_play || !check.integrity_verified {
        return Err(Error::State("cloud offline access failed"));
    }
    let suffix = path
        .extension()
        .and_then(|s| s.to_str())
        .ok_or(Error::State("cloud output suffix missing"))?
        .to_owned();
    let files = CompletedDownloads::new(reopened.clone())?;
    let renamed = files.rename(&completed.id, &format!("verified-cloud-original.{suffix}"))?;
    files.resume(&renamed.id)?;
    fs::create_dir(root.join("moved"))?;
    let moved = root
        .join("moved")
        .join(format!("verified-cloud-original.{suffix}"));
    let moving = files.start(&completed.id, FileAction::Move, Some(moved.clone()))?;
    files.resume(&moving.id)?;
    if inspect_file(&moved)? != integrity {
        return Err(Error::Media("cloud file management changed bytes"));
    }
    let deletion = files.start(&completed.id, FileAction::Delete, None)?;
    files.resume(&deletion.id)?;
    if moved.exists() || path.exists() || !offline.records(&reopened.token()?)?.is_empty() {
        return Err(Error::State("cloud local proof cleanup incomplete"));
    }
    let after = snapshot(&reopened).await?;
    let keys = |v: &cumulus_core::cloud::library::CloudSnapshot| {
        v.items
            .iter()
            .map(|e| (&e.song_id, &e.file_name, e.file_size))
            .map(|(id, name, size)| (id.clone(), name.clone(), size))
            .collect::<Vec<_>>()
    };
    if keys(&after) != keys(&baseline) {
        return Err(Error::State("cloud account baseline changed"));
    }
    Ok(
        json!({"ok":true,"baseline_count":baseline.items.len(),"baseline_preserved":true,"remote_account_mutations":0,"cloud_original_bytes":integrity.bytes,"local_sha256":integrity.sha256,"server_md5_provided":completed.resource.as_ref().is_some_and(|v|v["md5"].is_string()),"paused_at":131071,"reopened":true,"authorization_count":completed.authorization_count,"raw_bytes_preserved":true,"media":media,"full_decode":true,"offline_access":true,"renamed":true,"moved":true,"local_media_removed":true,"lyrics_status":completed.lyrics_status}),
    )
}
#[tokio::main]
async fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        eprintln!("usage: cloud_download_roundtrip EXISTING_PC_PROFILE NEW_PRIVATE_PROOF");
        std::process::exit(2);
    }
    let profile = PathBuf::from(&args[0]);
    let root = PathBuf::from(&args[1]);
    if !profile.join("core.sqlite3").is_file() || root.exists() {
        eprintln!("Existing PC profile and new proof directory required.");
        std::process::exit(2);
    }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&root).expect("create isolated proof");
    let result = run(&profile, &root).await;
    match result {
        Ok(value) => {
            fs::write(
                root.join("report.json"),
                serde_json::to_vec_pretty(&value).unwrap(),
            )
            .unwrap();
            println!(
                "Rust cloud original download, resume, decode and file-management proof passed; local media removed."
            );
        }
        Err(error) => {
            fs::write(root.join("private-error.txt"), error.to_string()).unwrap();
            eprintln!("Cloud download proof did not complete; inspect private evidence.");
            std::process::exit(1);
        }
    }
}
