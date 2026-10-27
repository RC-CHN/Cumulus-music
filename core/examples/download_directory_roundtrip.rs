//! Isolated fixture authorization; actual cross-filesystem moves, process exits,
//! range resumption and ffprobe. Does not load an account or make network calls.
#[path = "../tests/support/mod.rs"]
mod support;
use cumulus_core::{
    Error, Result,
    download::{
        completed::{CompletedDownloads, FileAction, FileEvent, FileOptions, FilePhase},
        directory::{DirectoryEvent, DirectoryOptions, DownloadDirectory, MigrationPhase},
        service::{
            DownloadAction, DownloadDestination, DownloadOptions, DownloadRunOptions,
            DownloadStatus,
        },
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command, sync::Arc};
fn wave() -> Vec<u8> {
    let size = 48000u32 * 16 * 4;
    let mut out = Vec::with_capacity(size as usize + 44);
    out.extend(b"RIFF");
    out.extend((size + 36).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(48000u32.to_le_bytes());
    out.extend(192000u32.to_le_bytes());
    out.extend(4u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(size.to_le_bytes());
    for n in 0..48000 * 16 {
        let sample = ((n as f64 * 440.0 * std::f64::consts::TAU / 48000.0).sin() * 1000.0) as i16;
        out.extend(sample.to_le_bytes());
        out.extend(sample.to_le_bytes());
    }
    out
}
fn child(args: &[String]) -> Result<Value> {
    if args.len() != 5 {
        return Err(Error::InvalidInput("invalid child arguments"));
    }
    let name = args[3].clone();
    let partial = args[4].clone();
    let batch_name = name.clone();
    let directory = DownloadDirectory::offline(Arc::new(StateStore::open(&args[1])?), "1")?
        .with_options(DirectoryOptions {
            files: FileOptions {
                observer: Some(Arc::new(move |event, op| {
                    let event = match event {
                        FileEvent::Prepared => "prepared",
                        FileEvent::WaitingForPlayback => "waiting_for_playback",
                        FileEvent::CopyChunk => "copy",
                        FileEvent::Staged => "staged",
                        FileEvent::DestinationReady => "ready",
                        FileEvent::MetadataCommitted => "commit",
                        FileEvent::SourceRemoved => "removed",
                        FileEvent::Finished => "finished",
                    };
                    if op.task_id == partial && event == name {
                        std::process::exit(73);
                    }
                    Ok(())
                })),
                ..Default::default()
            },
            observer: Some(Arc::new(move |event, _| {
                let event = match event {
                    DirectoryEvent::Intent => "intent",
                    DirectoryEvent::Planned => "planned",
                    DirectoryEvent::ItemComplete => "item",
                    DirectoryEvent::BeforeFinish => "before_finish",
                    DirectoryEvent::RollbackItem => "rollback",
                    DirectoryEvent::Finished => "batch_finished",
                };
                if event == batch_name {
                    std::process::exit(73);
                }
                Ok(())
            })),
            ..Default::default()
        });
    directory.resume(&args[2])?;
    Err(Error::State("child did not reach requested checkpoint"))
}
async fn run() -> Result<Value> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--crash-child") {
        return child(&args);
    }
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "usage: download_directory_roundtrip NEW_PROOF_DIRECTORY SECOND_FILESYSTEM_PARENT",
        ));
    }
    let root = PathBuf::from(&args[0]);
    fs::create_dir(&root)?;
    let root = root.canonicalize()?;
    let external = tempfile::Builder::new()
        .prefix("cumulus-directory-audit-")
        .tempdir_in(&args[1])?;
    #[cfg(unix)]
    let cross_device = {
        use std::os::unix::fs::MetadataExt;
        root.metadata()?.dev() != external.path().metadata()?.dev()
    };
    #[cfg(not(unix))]
    let cross_device =
        root.components().next() != external.path().canonicalize()?.components().next();
    if !cross_device {
        return Err(Error::InvalidInput(
            "audit requires two distinct filesystems",
        ));
    }
    let bytes = wave();
    let mut checks = Vec::new();
    for (name, rollback) in [
        ("prepared", false),
        ("copy", false),
        ("staged", false),
        ("ready", false),
        ("commit", false),
        ("removed", false),
        ("finished", false),
        ("planned", false),
        ("item", false),
        ("before_finish", false),
        ("copy", true),
        ("commit", true),
    ] {
        let case = root.join(format!(
            "{name}-{}",
            if rollback { "cancel" } else { "resume" }
        ));
        fs::create_dir(&case)?;
        let (service, _, store, api, backend) =
            support::download::setup(&case, bytes.clone(), "wav");
        let source = case.join("media");
        let target = external.path().join(case.file_name().expect("case"));
        let directory = DownloadDirectory::offline(store.clone(), "1")?;
        directory.configure(&source, 2, 1)?;
        let options = DownloadOptions {
            tags: false,
            cover: false,
            lyrics: true,
            ..Default::default()
        };
        let done = service.create(
            &json!(123),
            DownloadDestination::Path {
                path: source.join("album/done.wav"),
            },
            options.clone(),
        )?;
        service.run(&done.id, DownloadRunOptions::default()).await?;
        let partial = service.create(
            &json!(123),
            DownloadDestination::Path {
                path: source.join("partial.wav"),
            },
            options.clone(),
        )?;
        service
            .run(
                &partial.id,
                DownloadRunOptions {
                    byte_budget: Some(123457),
                    ..Default::default()
                },
            )
            .await?;
        let queued = service.create_default(&json!(123), options)?;
        let batch = directory.start(&target, None)?;
        let api_calls = api
            .calls
            .lock()
            .map_err(|_| Error::State("fixture lock"))?
            .len();
        let status = Command::new(std::env::current_exe()?)
            .arg("--crash-child")
            .arg(store.directory())
            .arg(&batch.id)
            .arg(name)
            .arg(&partial.id)
            .status()?;
        if status.code() != Some(73) {
            return Err(Error::State("child did not exit at a durable checkpoint"));
        }
        let reopened =
            DownloadDirectory::offline(Arc::new(StateStore::open(store.directory())?), "1")?;
        let view = if rollback {
            reopened.cancel(&batch.id)?
        } else {
            reopened.resume(&batch.id)?
        };
        if view.phase
            != if rollback {
                MigrationPhase::Cancelled
            } else {
                MigrationPhase::Complete
            }
            || reopened.active()?.is_some()
            || api
                .calls
                .lock()
                .map_err(|_| Error::State("fixture lock"))?
                .len()
                != api_calls
        {
            return Err(Error::State(
                "directory did not recover without API requests",
            ));
        }
        let result_root = if rollback { &source } else { &target };
        if reopened
            .settings()?
            .ok_or(Error::State("missing settings"))?
            .root
            != *result_root
        {
            return Err(Error::State("wrong default directory"));
        }
        if service
            .run(&partial.id, DownloadRunOptions::default())
            .await?
            .status
            != DownloadStatus::Complete
            || backend.audio.calls().last().map(|r| r.offset) != Some(123457)
        {
            return Err(Error::State(
                "download did not resume at the original range",
            ));
        }
        for relative in ["album/done.wav", "partial.wav"] {
            let path = result_root.join(relative);
            if fs::read(&path)? != bytes || !path.with_extension("lrc").is_file() {
                return Err(Error::State("migrated audio or lyrics differ"));
            }
        }
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name,sample_rate,channels,duration",
                "-of",
                "json",
            ])
            .arg(result_root.join("partial.wav"))
            .output()?;
        if !output.status.success() {
            return Err(Error::State("ffprobe failed"));
        }
        let probe: Value = serde_json::from_slice(&output.stdout)?;
        if probe["streams"][0]["codec_name"] != "pcm_s16le"
            || probe["streams"][0]["duration"] != "16.000000"
        {
            return Err(Error::State("unexpected decoded media"));
        }
        let files = CompletedDownloads::offline(store.clone(), "1")?;
        for id in [&done.id, &partial.id] {
            let op = files.start(id, FileAction::Delete, None)?;
            if files.resume(&op.id)?.phase != FilePhase::Complete {
                return Err(Error::State("fixture media cleanup failed"));
            }
        }
        service.request(&queued.id, DownloadAction::Cancel)?;
        for relative in ["album/done.wav", "partial.wav"] {
            let p = result_root.join(relative);
            if p.exists() || p.with_extension("lrc").exists() {
                return Err(Error::State("fixture media remains"));
            }
        }
        checks.push(json!({"checkpoint":name,"rollback":rollback,"exit_code":73,"range_offset":123457,"decoded_seconds":16,"migrated_tasks":view.items.len(),"ok":true}));
    }
    Ok(
        json!({"ok":true,"authorization":"fixture_only","network_requests":0,"cross_device":cross_device,"wav_bytes":bytes.len(),"checkpoints":checks,"media_and_sidecars_removed":true}),
    )
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    match run().await {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).expect("JSON")),
        Err(e) => {
            eprintln!("directory audit failed: {e}");
            std::process::exit(1);
        }
    }
}
