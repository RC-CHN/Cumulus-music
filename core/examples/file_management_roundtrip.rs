//! Explicit local functional audit: fixture authorization, real Rust download,
//! filesystem, process exits, ffprobe and libmpv. No account/network access.
#[path = "../tests/support/mod.rs"]
mod support;
use cumulus_core::{
    Error, Result,
    download::{
        completed::*,
        service::{DownloadDestination, DownloadOptions, DownloadRunOptions, DownloadStatus},
    },
    library::{LocalLibrary, identity::file_id},
    media::integrity::inspect_file,
    playback::{
        controller::{
            PlaybackCommand as Cmd, PlaybackController, PlaybackSnapshot, PlaybackStatus,
        },
        engine::{MpvConfig, MpvEngine},
        source::SourceResolver,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{path::PathBuf, process::Command, sync::Arc, time::Duration};

fn wave() -> Vec<u8> {
    let bytes = 48000u32 * 8 * 4;
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((bytes + 36).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(48000u32.to_le_bytes());
    out.extend(192000u32.to_le_bytes());
    out.extend(4u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(bytes.to_le_bytes());
    for n in 0..48000 * 8 {
        let value = ((n as f64 * 440.0 * std::f64::consts::TAU / 48000.0).sin() * 1000.0) as i16;
        out.extend(value.to_le_bytes());
        out.extend(value.to_le_bytes());
    }
    out
}
fn event(name: &str) -> Result<FileEvent> {
    Ok(match name {
        "copy" => FileEvent::CopyChunk,
        "staged" => FileEvent::Staged,
        "ready" => FileEvent::DestinationReady,
        "commit" => FileEvent::MetadataCommitted,
        "removed" => FileEvent::SourceRemoved,
        _ => return Err(Error::InvalidInput("invalid checkpoint")),
    })
}
async fn wait(
    player: &PlaybackController,
    predicate: impl Fn(&PlaybackSnapshot) -> bool,
) -> Result<PlaybackSnapshot> {
    let mut receiver = player.subscribe();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let value = receiver.borrow().clone();
            if value.state == PlaybackStatus::Error {
                return Err(Error::State("playback proof error"));
            }
            if predicate(&value) {
                return Ok(value);
            }
            receiver
                .changed()
                .await
                .map_err(|_| Error::State("player stopped"))?;
        }
    })
    .await
    .map_err(|_| Error::State("player proof timeout"))?
}
async fn run() -> Result<Value> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|s| s == "--crash-child") {
        if args.len() != 4 {
            return Err(Error::InvalidInput("invalid child arguments"));
        }
        let checkpoint = event(&args[3])?;
        let files = CompletedDownloads::offline(Arc::new(StateStore::open(&args[1])?), "1")?
            .with_options(FileOptions {
                observer: Some(Arc::new(move |seen, _| {
                    if seen == checkpoint {
                        std::process::exit(73);
                    }
                    Ok(())
                })),
                ..Default::default()
            });
        files.resume(&args[2])?;
        return Err(Error::State("child did not reach crash checkpoint"));
    }
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "usage: file_management_roundtrip NEW_PROOF_DIRECTORY SECOND_FILESYSTEM_PARENT",
        ));
    }
    let root = PathBuf::from(&args[0]);
    std::fs::create_dir(&root)?;
    let external = tempfile::Builder::new()
        .prefix("cumulus-file-audit-")
        .tempdir_in(&args[1])?;
    #[cfg(unix)]
    let cross_device = {
        use std::os::unix::fs::MetadataExt;
        root.metadata()?.dev() != external.path().metadata()?.dev()
    };
    #[cfg(not(unix))]
    let cross_device = root.canonicalize()?.components().next()
        != external.path().canonicalize()?.components().next();
    if !cross_device {
        return Err(Error::InvalidInput(
            "audit requires two distinct filesystems",
        ));
    }
    let bytes = wave();
    let mut checkpoints = 0;
    for cross in [false, true] {
        for name in ["copy", "staged", "ready", "commit", "removed"] {
            if !cross && matches!(name, "copy" | "staged") {
                continue;
            }
            let case = root.join(format!("{}-{name}", if cross { "cross" } else { "same" }));
            std::fs::create_dir(&case)?;
            let (service, _, store, _, _) = support::download::setup(&case, bytes.clone(), "wav");
            let source = case.join("media/a.wav");
            let task = service.create(
                &json!(123),
                DownloadDestination::Path {
                    path: source.clone(),
                },
                DownloadOptions {
                    tags: false,
                    cover: false,
                    lyrics: true,
                    ..Default::default()
                },
            )?;
            let done = service.run(&task.id, DownloadRunOptions::default()).await?;
            if done.status != DownloadStatus::Complete || done.cleanup_pending {
                return Err(Error::State("fixture download incomplete"));
            }
            let target = if cross {
                external.path().join(format!("{name}/moved.wav"))
            } else {
                case.join("target/moved.wav")
            };
            let files = CompletedDownloads::offline(store.clone(), "1")?;
            let op = files.start(&task.id, FileAction::Move, Some(target.clone()))?;
            let status = Command::new(std::env::current_exe()?)
                .arg("--crash-child")
                .arg(store.directory())
                .arg(&op.id)
                .arg(name)
                .status()?;
            if status.code() != Some(73) {
                return Err(Error::State("child did not exit at durable checkpoint"));
            }
            let reopened =
                CompletedDownloads::offline(Arc::new(StateStore::open(store.directory())?), "1")?;
            if reopened.resume(&op.id)?.phase != FilePhase::Complete
                || source.exists()
                || std::fs::read(&target)? != bytes
                || !target.with_extension("lrc").is_file()
            {
                return Err(Error::State("crash recovery differs from original media"));
            }
            let removed = reopened.start(&task.id, FileAction::Delete, None)?;
            reopened.resume(&removed.id)?;
            if target.exists() || target.with_extension("lrc").exists() {
                return Err(Error::State("isolated cleanup incomplete"));
            }
            checkpoints += 1;
        }
    }
    let case = root.join("player");
    std::fs::create_dir(&case)?;
    let (service, session, store, api, _) = support::download::setup(&case, bytes.clone(), "wav");
    let source = case.join("media/current.wav");
    let second = case.join("media/next.wav");
    let task = service.create(
        &json!(123),
        DownloadDestination::Path {
            path: source.clone(),
        },
        DownloadOptions {
            tags: false,
            cover: false,
            lyrics: true,
            ..Default::default()
        },
    )?;
    service.run(&task.id, DownloadRunOptions::default()).await?;
    std::fs::write(&second, &bytes)?;
    let library = Arc::new(LocalLibrary::new(store.clone())?);
    library.scan(source.parent().expect("parent"))?;
    let old_id = file_id(&source)?;
    let resource = serde_json::to_value(library.resource(&old_id)?)?;
    let resolver = Arc::new(SourceResolver::new(session.clone(), Some(library.clone())));
    let engine = MpvEngine::open(MpvConfig {
        null_output: true,
        ..Default::default()
    })?;
    let player =
        PlaybackController::start(session.clone(), store.clone(), resolver, Box::new(engine))?;
    let token = session.token()?;
    player.command(&token, Cmd::Volume(0.0)).await?;
    player
        .command(
            &token,
            Cmd::Replace {
                resources: vec![resource, json!({"path":second})],
                index: 0,
                source: json!({"kind":"file-audit"}),
            },
        )
        .await?;
    wait(&player, |s| s.state == PlaybackStatus::Playing).await?;
    player.command(&token, Cmd::Pause(true)).await?;
    player.command(&token, Cmd::Seek(2.0)).await?;
    wait(&player, |s| {
        s.state == PlaybackStatus::Paused && s.position.is_some_and(|n| (n - 2.0).abs() < 0.15)
    })
    .await?;
    // Observe settled native time, beyond the immediate requested seek value.
    tokio::time::sleep(Duration::from_millis(700)).await;
    let before = player.command(&token, Cmd::Snapshot).await?;
    let target = external.path().join("playing/renamed.wav");
    let files = CompletedDownloads::new(session.clone())?;
    let call_count = api.calls.lock().unwrap().len();
    let op = files.start(&task.id, FileAction::Move, Some(target.clone()))?;
    let worker = files.clone();
    tokio::task::spawn_blocking(move || worker.resume(&op.id))
        .await
        .map_err(|_| Error::State("file worker stopped"))??;
    let new_id = file_id(&target)?;
    wait(&player, |s| {
        s.state == PlaybackStatus::Paused
            && s.play_id != before.play_id
            && s.selected
                .as_ref()
                .is_some_and(|e| e.resource["local_id"] == new_id)
    })
    .await?;
    tokio::time::sleep(Duration::from_millis(700)).await;
    let moved = player.command(&token, Cmd::Snapshot).await?;
    if !moved
        .position
        .zip(before.position)
        .is_some_and(|(a, b)| (a - b).abs() < 0.05)
        || moved.selected.as_ref().map(|e| &e.entry_id)
            != before.selected.as_ref().map(|e| &e.entry_id)
    {
        eprintln!(
            "{}",
            json!({"stage":"move_playback","before_seconds":before.position,"after_seconds":moved.position,"same_entry":moved.selected.as_ref().map(|e|&e.entry_id)==before.selected.as_ref().map(|e|&e.entry_id)})
        );
        return Err(Error::State("move lost playback position or entry"));
    }
    if inspect_file(&target)?.bytes != bytes.len() as u64 || source.exists() {
        return Err(Error::State("cross-device media proof mismatch"));
    }
    let op = files.start(&task.id, FileAction::Delete, None)?;
    let worker = files.clone();
    tokio::task::spawn_blocking(move || worker.resume(&op.id))
        .await
        .map_err(|_| Error::State("file worker stopped"))??;
    let next = wait(&player, |s| {
        s.state == PlaybackStatus::Paused
            && s.selected
                .as_ref()
                .is_some_and(|e| e.resource["path"] == json!(second))
    })
    .await?;
    if next.queue_len != 1
        || !next.position.is_some_and(|n| n < 0.15)
        || api.calls.lock().unwrap().len() != call_count
    {
        return Err(Error::State(
            "delete changed pause, queue, position or requested network",
        ));
    }
    player.command(&token, Cmd::Stop).await?;
    drop(player);
    std::fs::remove_file(second)?;
    if target.exists() || target.with_extension("lrc").exists() {
        return Err(Error::State("playback file cleanup incomplete"));
    }
    Ok(
        json!({"ok":true,"authorization":"fixture_only","network_requests":0,"cross_device":cross_device,"crash_checkpoints":checkpoints,"wav_bytes":bytes.len(),"native_decoder":"libmpv","output":"null","requested_seek_seconds":2,"native_before_move_seconds":before.position,"native_after_move_seconds":moved.position,"position_tolerance_seconds":0.05,"delete_advanced_paused":true,"media_and_sidecars_removed":true}),
    )
}
#[tokio::main]
async fn main() {
    match run().await {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => {
            eprintln!("file audit failed: {e}");
            std::process::exit(1);
        }
    }
}
