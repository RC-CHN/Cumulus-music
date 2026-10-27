//! Explicit isolated real-account proof; never executed by tests/startup.
//! Publishes only a newly synthesized private cloud item, then removes exactly
//! its new ID after matching baseline exclusion, unique filename and file size.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    cloud::{CloudApi, upload::*},
    protocol::{
        api::identifier,
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
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
async fn list(session: &AccountSession) -> Result<Vec<Value>> {
    session
        .run_for(&session.token()?, |api, _| async move {
            CloudApi::new(api).list().await
        })
        .await
}
fn ids(items: &[Value]) -> Result<BTreeSet<String>> {
    items.iter().map(|v| identifier(&v["songId"])).collect()
}
fn synthesize(root: &Path, name: &str) -> Result<PathBuf> {
    let wav = root.join("input.wav");
    let output = root.join(format!("{name}.flac"));
    let samples = 44100u32 * 10;
    let data_len = samples * 2;
    let mut bytes = Vec::with_capacity(data_len as usize + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&44100u32.to_le_bytes());
    bytes.extend_from_slice(&88200u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    let seed = uuid::Uuid::new_v4().as_u128() as u64;
    let mut random = seed;
    for index in 0..samples {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let tone = (index as f64 * std::f64::consts::TAU * (320.0 + (seed % 300) as f64) / 44100.0)
            .sin()
            * 2800.0;
        let noise = ((random >> 48) as i16 as f64) * 0.08;
        bytes.extend_from_slice(&((tone + noise) as i16).to_le_bytes());
    }
    write_new(&wav, &bytes)?;
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-n", "-i"])
        .arg(&wav)
        .args(["-c:a", "flac", "-metadata"])
        .arg(format!("title={name}"))
        .args([
            "-metadata",
            "artist=Cumulus isolated verification",
            "-metadata",
            "album=Rust core proof",
        ])
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(Error::Media("isolated FLAC generation failed"));
    }
    fs::remove_file(wav)?;
    Ok(output)
}
fn manager(session: Arc<AccountSession>) -> Result<CloudUploads> {
    let mut adapters = UploadAdapters::native()?;
    adapters.chunk_size = 8192;
    CloudUploads::with_adapters(session, adapters)
}
async fn cleanup(
    session: &AccountSession,
    store: &StateStore,
    owner: &str,
    task: &CloudView,
    baseline: &BTreeSet<String>,
    filename: &str,
    size: u64,
) -> Result<bool> {
    let current = list(session).await?;
    let Some(id) = task.song_id.as_ref() else {
        return Ok(ids(&current)? == *baseline);
    };
    if baseline.contains(id) {
        return Err(Error::State(
            "cloud proof ID was already present; refusing deletion",
        ));
    }
    if let Some(item) = current
        .iter()
        .find(|v| identifier(&v["songId"]).ok().as_ref() == Some(id))
    {
        if item["fileName"].as_str() != Some(filename) || item["fileSize"].as_u64() != Some(size) {
            return Err(Error::State("isolated cloud cleanup identity mismatch"));
        }
        store.put(
            owner,
            "cloud_proof_v1",
            "cleanup",
            &json!({"song_id":id,"phase":"delete_pending"}),
        )?;
        let song_id = id.clone();
        let deletion = session
            .run_for(&session.token()?, |api, _| async move {
                CloudApi::new(api).delete(&[json!(song_id)]).await
            })
            .await;
        // A lost delete reply is reconciled read-only, never sent twice.
        let after = list(session).await?;
        let restored = ids(&after)? == *baseline;
        store.put(owner,"cloud_proof_v1","cleanup",&json!({"song_id":id,"phase":if restored{"restored"}else{"unresolved"},"reply_accepted":deletion.is_ok()}))?;
        Ok(restored)
    } else {
        Ok(ids(&current)? == *baseline)
    }
}
async fn run() -> Result<Value> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "usage: cloud_upload_roundtrip EXISTING_PC_PROFILE NEW_PRIVATE_PROOF_DIRECTORY",
        ));
    }
    let profile = PathBuf::from(&args[0]);
    let root = PathBuf::from(&args[1]);
    if !profile.join("core.sqlite3").is_file() || root.exists() {
        return Err(Error::InvalidInput(
            "existing profile and new proof directory required",
        ));
    }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&root)?;
    let state = root.join("state");
    let session = account(&profile, &state).await?;
    let token = session.token()?;
    let owner = token
        .uid
        .as_deref()
        .ok_or(Error::State("missing cloud proof account"))?;
    let store = StateStore::open(&state)?;
    let baseline = ids(&list(&session).await?)?;
    let name = format!("CumulusRustCloud{}", uuid::Uuid::new_v4().simple());
    let input = synthesize(&root, &name)?;
    let size = fs::metadata(&input)?.len();
    let filename = input
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or(Error::State("missing fixture name"))?
        .to_owned();
    let cover = root.join("cover.png");
    image::RgbaImage::from_pixel(32, 32, image::Rgba([35, 105, 173, 255]))
        .save(&cover)
        .map_err(|_| Error::Media("isolated cover generation failed"))?;
    let service = manager(session.clone())?;
    let created = service
        .create(
            &input,
            AttachmentInputs {
                cover: Some(cover.clone()),
                ..Default::default()
            },
        )
        .await?;
    let id = created.id;
    store.put(owner,"cloud_proof_v1","baseline",&json!({"ids":baseline,"task_id":id,"file_name":filename,"bytes":size,"cleanup_required":true}))?;
    let mut checks = Vec::new();
    let mut last = service.get(&id)?;
    let outcome: Result<()> = async {
        let paused = service
            .run(
                &id,
                CloudRunOptions {
                    chunk_budget: Some(1),
                    ..Default::default()
                },
            )
            .await?;
        if paused.phase != CloudPhase::AudioUploading
            || paused.bytes != 8192
            || paused.bytes >= paused.total
        {
            return Err(Error::State(
                "cloud proof did not pause in NOS audio transfer",
            ));
        }
        checks.push("audio_first_chunk_pause");
        service.renew_token(&id).await?;
        checks.push("same_object_token_renewal");
        let reopened = manager(account(&profile, &state).await?)?;
        for _ in 0..40 {
            last = reopened.run(&id, Default::default()).await?;
            eprintln!("cloud proof phase {:?}", last.phase);
            if last.phase == CloudPhase::Complete {
                checks.extend([
                    "database_reopen_resume",
                    "cover_uploaded",
                    "encoding_ready",
                    "private_cloud_published",
                    "own_account_readback",
                ]);
                return Ok(());
            }
            if last.phase == CloudPhase::Failed {
                return Err(Error::State("cloud encoding failed"));
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| Error::State("invalid clock"))?
                .as_millis() as u64;
            tokio::time::sleep(Duration::from_millis(
                last.next_check_ms.saturating_sub(now).clamp(500, 5000),
            ))
            .await;
        }
        Err(Error::State(
            "cloud proof still waiting for service encoding/readback",
        ))
    }
    .await;
    // Reopen authoritative task facts even if a response or parent commit was
    // lost; do not infer cleanup identity from a guessed title/song match.
    let cleanup_session = account(&profile, &state).await?;
    let cleanup_service = manager(cleanup_session.clone())?;
    last = cleanup_service.get(&id)?;
    if last.phase == CloudPhase::PublicationPending {
        last = cleanup_service.reconcile_publication(&id).await?;
    }
    let cleaned = cleanup(
        &cleanup_session,
        &store,
        owner,
        &last,
        &baseline,
        &filename,
        size,
    )
    .await;
    let restored = matches!(cleaned, Ok(true));
    if restored {
        checks.push("baseline_restored");
    }
    let report = json!({"schema":1,"implementation":"rust-core","success":outcome.is_ok()&&restored,
        "checks":checks,"baseline_count":baseline.len(),"baseline_restored":restored,
        "phase":last.phase,"channel":last.channel,"audio_bytes":last.bytes,"audio_total":last.total,
        "attachments":last.attachments.iter().map(|a|json!({"kind":a.kind,"phase":a.phase,"bytes":a.bytes,"total":a.total})).collect::<Vec<_>>(),
        "error":outcome.err().map(|e|e.to_string()),"cleanup_error":cleaned.err().map(|e|e.to_string()),
        "cleanup_required":!restored,"existing_resources_deleted":0});
    write_new(
        &root.join("report.json"),
        serde_json::to_string_pretty(&report)?.as_bytes(),
    )?;
    if restored {
        fs::remove_file(input)?;
        fs::remove_file(cover)?;
    }
    Ok(report)
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    match run().await {
        Ok(report) => {
            println!("{}", serde_json::to_string_pretty(&report).expect("report"));
            if report["success"] != true {
                std::process::exit(1)
            }
        }
        Err(error) => {
            eprintln!("cloud proof failed: {error}; inspect its private journal before any retry");
            std::process::exit(1)
        }
    }
}
