//! Current-account Rust proof. Reversible collection test only touches an MV
//! absent from the complete baseline; its private restore intent precedes writes.
//! Downloads/decoding operate only inside a newly created private directory.
use cumulus_core::{
    Error, Result,
    account::{
        AccountSession,
        credentials::Credentials,
        resources::{AccountResources, ResourceKind},
    },
    download::{
        completed::{CompletedDownloads, FileAction},
        offline::OfflineLibrary,
        service::{DownloadDestination, DownloadRunOptions, DownloadService, DownloadStatus},
    },
    media::integrity::inspect_file,
    mv::{MvPurpose, MvService},
    playback::engine::{AudioEngine, Cancellation, LoadRequest, MediaInput, MpvConfig, MpvEngine},
    protocol::{
        api::identifier,
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
async fn account(profile: &Path, root: &Path) -> Result<Arc<AccountSession>> {
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let api = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
    let session = Arc::new(AccountSession::new(
        api,
        Some(Arc::new(StateStore::open(root.join("state"))?)),
    ));
    session.refresh().await?;
    Ok(session)
}
fn save(root: &Path, name: &str, value: &Value) -> Result<()> {
    use std::io::Write;
    let mut file = fs::File::create(root.join(name))?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    Ok(())
}
fn ids(value: &Value) -> Result<Vec<String>> {
    value
        .as_array()
        .ok_or(Error::Response("MV collection missing"))?
        .iter()
        .map(|v| identifier(&v["id"]))
        .collect()
}
async fn restore(session: Arc<AccountSession>, root: &Path) -> Result<usize> {
    let intent: Value = serde_json::from_slice(&fs::read(root.join("restore-intent.json"))?)?;
    let id = identifier(&intent["id"])?;
    if session.token()?.uid.as_deref() != intent["owner"].as_str()
        || intent["originally_present"] != false
    {
        return Err(Error::State("MV cleanup owner or original state mismatch"));
    }
    let baseline: Vec<String> = serde_json::from_value(intent["baseline_ids"].clone())?;
    if baseline.contains(&id) {
        return Err(Error::State("cannot remove an original MV collection item"));
    }
    let resources = AccountResources::new(session)?;
    let current = ids(&resources.refresh(ResourceKind::Mvs).await?)?;
    if current.contains(&id) {
        if resources
            .operations()?
            .get(&format!("mvs:{id}"))
            .is_some_and(|v| v.phase.pending())
        {
            resources.reconcile(ResourceKind::Mvs, &id).await?;
        }
        resources
            .set_membership(ResourceKind::Mvs, &json!(id), false)
            .await?;
    }
    let after = ids(&resources.refresh(ResourceKind::Mvs).await?)?;
    if after != baseline {
        return Err(Error::State(
            "MV collection order or membership differs from baseline",
        ));
    }
    save(
        root,
        "restored.json",
        &json!({"baseline_preserved":true,"baseline_count":after.len()}),
    )?;
    Ok(after.len())
}
fn play_local(path: PathBuf) -> Result<Value> {
    let mut engine = MpvEngine::open(MpvConfig {
        library: None,
        null_output: true,
    })?;
    let media = engine.load(
        LoadRequest::new(MediaInput::File(path)),
        &Cancellation::default(),
    )?;
    engine.pause(false)?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let progressed = loop {
        let state = engine.poll()?;
        if state.position.is_some_and(|p| p > 0.4) {
            break state.position.unwrap();
        }
        if Instant::now() >= deadline {
            return Err(Error::Media("MV local decode did not progress"));
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    engine.pause(true)?;
    engine.seek(10.0)?;
    let deadline = Instant::now() + Duration::from_secs(8);
    let position = loop {
        let state = engine.poll()?;
        if state.paused && state.position.is_some_and(|v| (v - 10.0).abs() < 0.6) {
            break state.position.unwrap();
        }
        if Instant::now() >= deadline {
            return Err(Error::Media("MV paused seek failed"));
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    engine.stop()?;
    Ok(
        json!({"null_output":true,"visible_video_verified":false,"duration":media.duration,"codec":media.codec,"progressed_seconds":progressed,"paused_seek_seconds":position,"api_version":engine.api_version()}),
    )
}
async fn run(profile: &Path, sample: &Path, root: &Path) -> Result<Value> {
    let old: Value = serde_json::from_slice(&fs::read(sample)?)?;
    let id = identifier(&old["data"]["id"])?;
    let session = account(profile, root).await?;
    let token = session.token()?;
    let resources = AccountResources::new(session.clone())?;
    let baseline = ids(&resources.refresh(ResourceKind::Mvs).await?)?;
    if baseline.contains(&id) {
        return Err(Error::State(
            "sample is already collected; preserve original collection",
        ));
    }
    let mv = MvService::new(session.clone())?;
    let detail = mv.detail_for(&token, &json!(id)).await?;
    if detail.id != id
        || detail
            .privilege
            .as_ref()
            .is_none_or(|p| p.fee.unwrap_or(0) != 0 || p.unauthorized)
    {
        return Err(Error::State("proof requires a free MV sample"));
    }
    let resolution = detail
        .download_options()
        .into_iter()
        .filter(|v| v.point.unwrap_or(0) == 0)
        .filter(|v| v.size.is_some_and(|s| s > 131071 && s <= 128 * 1024 * 1024))
        .map(|v| v.br)
        .min()
        .ok_or(Error::State("no free MV resolution"))?;
    let play = mv
        .authorize_for(&token, &json!(id), resolution, MvPurpose::Play)
        .await?;
    let play_resolution = play["r"].clone();
    drop(play);
    let service = DownloadService::new(session.clone())?;
    let task = service
        .create_mv(
            &json!(id),
            resolution,
            DownloadDestination::Named {
                root: root.join("media"),
                path_type: 0,
                file_type: 1,
            },
        )
        .await?;
    let paused = service
        .run(
            &task.id,
            DownloadRunOptions {
                byte_budget: Some(131071),
                ..Default::default()
            },
        )
        .await?;
    if paused.status != DownloadStatus::Paused || paused.bytes != 131071 {
        return Err(Error::State("MV pause checkpoint failed"));
    }
    save(
        root,
        "paused.json",
        &json!({"bytes":paused.bytes,"authorization_count":paused.authorization_count}),
    )?;
    drop((mv, resources, service, session));
    let session = account(profile, root).await?;
    let service = DownloadService::new(session.clone())?;
    service.enqueue(&task.id)?;
    let completed = service.run(&task.id, DownloadRunOptions::default()).await?;
    if completed.status != DownloadStatus::Complete
        || completed.encrypted
        || !completed.origin.is_mv()
    {
        return Err(Error::State("MV download incomplete"));
    }
    let path = completed
        .target
        .as_ref()
        .ok_or(Error::State("MV output missing"))?;
    let digest = inspect_file(path)?;
    let raw = completed
        .raw_integrity
        .as_ref()
        .ok_or(Error::State("MV raw digest missing"))?;
    if raw.sha256 != digest.sha256 || raw.bytes != digest.bytes {
        return Err(Error::Media("MV output differs from raw authorized bytes"));
    }
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type,codec_name,width,height,sample_rate,channels",
            "-of",
            "json",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .output()?;
    if !probe.status.success() {
        return Err(Error::Media("MV ffprobe failed"));
    }
    let media: Value = serde_json::from_slice(&probe.stdout)?;
    if !media["streams"].as_array().is_some_and(|rows| {
        rows.iter().any(|v| v["codec_type"] == "video")
            && rows.iter().any(|v| v["codec_type"] == "audio")
    }) {
        return Err(Error::Media("MV requires video and audio streams"));
    }
    let decode = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(path)
        .args(["-f", "null", "-"])
        .stdin(Stdio::null())
        .output()?;
    if !decode.status.success() {
        return Err(Error::Media("MV full audio/video decode failed"));
    }
    let input = path.clone();
    let playback = tokio::task::spawn_blocking(move || play_local(input))
        .await
        .map_err(|_| Error::State("MV player worker failed"))??;
    let mv = MvService::new(session.clone())?;
    let position = mv.save_position_for(
        &session.token()?,
        &json!(id),
        10_000,
        detail.duration_ms,
        false,
    )?;
    let check_session = account(profile, root).await?;
    if MvService::new(check_session.clone())?.position_for(&check_session.token()?, &json!(id))?
        != Some(position)
    {
        return Err(Error::State("MV position reopen mismatch"));
    }
    drop(check_session);
    let offline = OfflineLibrary::new(session.clone())?;
    let check = offline.check(&session.token()?, path, true).await?;
    if !check.access.can_play || !check.integrity_verified {
        return Err(Error::State("MV offline registration failed"));
    }
    let files = CompletedDownloads::new(session.clone())?;
    let rename = files.rename(&task.id, "verified-mv.mp4")?;
    files.resume(&rename.id)?;
    fs::create_dir(root.join("moved"))?;
    let moved = root.join("moved/verified-mv.mp4");
    let op = files.start(&task.id, FileAction::Move, Some(moved.clone()))?;
    files.resume(&op.id)?;
    if inspect_file(&moved)? != digest {
        return Err(Error::Media("MV managed move changed bytes"));
    }
    let op = files.start(&task.id, FileAction::Delete, None)?;
    files.resume(&op.id)?;
    if moved.exists() || !offline.records(&session.token()?)?.is_empty() {
        return Err(Error::State("MV local media cleanup incomplete"));
    }
    save(
        root,
        "download-verified.json",
        &json!({"bytes":digest.bytes,"sha256":digest.sha256,"full_decode":true,"local_media_removed":true}),
    )?;
    // All account mutations follow a durable private restore intent. This test
    // never spends points or calls a payment endpoint.
    let resources = AccountResources::new(session.clone())?;
    if ids(&resources.refresh(ResourceKind::Mvs).await?)? != baseline {
        return Err(Error::State("MV baseline changed before reversible test"));
    }
    save(
        root,
        "restore-intent.json",
        &json!({"owner":session.token()?.uid,"id":id,"originally_present":false,"baseline_ids":baseline}),
    )?;
    let mutation = resources
        .set_membership(ResourceKind::Mvs, &json!(id), true)
        .await;
    let restored = restore(session, root).await;
    mutation?;
    let baseline_count = restored?;
    Ok(
        json!({"ok":true,"baseline_count":baseline_count,"baseline_preserved":true,"collection_roundtrip":true,"point_spending":0,"download_resolution_requested":resolution,"download_resolution_actual":completed.resource.as_ref().map(|v|&v["r"]),"play_authorization_resolution":play_resolution,"bytes":digest.bytes,"raw_output_sha256":digest.sha256,"pause_bytes":paused.bytes,"authorization_count":completed.authorization_count,"full_decode":true,"media":media,"mpv":playback,"position_reopened":true,"offline_verified":true,"rename_move_delete":true,"local_media_removed":true,"official_detail_normalized":true}),
    )
}
#[tokio::main]
async fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        eprintln!(
            "usage: mv_roundtrip EXISTING_PC_PROFILE PRIVATE_SAMPLE_DETAIL NEW_PRIVATE_PROOF\n       mv_roundtrip EXISTING_PC_PROFILE --restore EXISTING_PRIVATE_PROOF"
        );
        std::process::exit(2);
    }
    let profile = PathBuf::from(&args[0]);
    let root = PathBuf::from(&args[2]);
    if !profile.join("core.sqlite3").is_file() {
        eprintln!("Existing PC profile required");
        std::process::exit(2);
    }
    let result = if args[1] == "--restore" {
        match account(&profile, &root).await {
            Ok(session) => restore(session, &root)
                .await
                .map(|count| json!({"restored":true,"baseline_count":count})),
            Err(e) => Err(e),
        }
    } else {
        if root.exists() {
            eprintln!("A new isolated proof directory is required");
            std::process::exit(2);
        }
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        if builder.create(&root).is_err() {
            eprintln!("Cannot create private proof directory");
            std::process::exit(2);
        }
        run(&profile, &PathBuf::from(&args[1]), &root).await
    };
    match result {
        Ok(report) => {
            save(&root, "report.json", &report).expect("save report");
            println!("Rust MV proof passed; inspect sanitized report. No points spent.");
        }
        Err(e) => {
            let _ = fs::write(root.join("private-error.txt"), e.to_string());
            eprintln!(
                "MV proof did not complete; inspect private evidence and restore intent if present."
            );
            std::process::exit(1);
        }
    }
}
