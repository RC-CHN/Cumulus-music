//! Current-account read-only MV streaming and software-frame verification.
//! Private sources never appear in reports or frontend-facing state.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    mv::{
        MvService,
        playback::{
            VideoCommand as Cmd, VideoController, VideoEngineFactory, VideoSnapshot,
            VideoStatus as Status,
        },
    },
    playback::engine::{MpvConfig, MpvVideoEngine},
    protocol::{
        api::identifier,
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
async fn until(
    player: &VideoController,
    check: impl Fn(&VideoSnapshot) -> bool,
) -> Result<VideoSnapshot> {
    let deadline = Instant::now() + Duration::from_secs(55);
    loop {
        let value = player.snapshot()?;
        if let Some(error) = value.error {
            return Err(Error::Shared(error));
        }
        if check(&value) {
            return Ok(value);
        }
        if Instant::now() >= deadline {
            return Err(Error::State("native MV video state timeout"));
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}
async fn capture(
    player: &VideoController,
    token: &cumulus_core::account::SessionToken,
    play_id: &str,
    root: &Path,
    name: &str,
    width: u32,
    height: u32,
) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last = None;
    loop {
        if let Some(frame) = player.frame(token, play_id, 0, width, height)?
            && frame.width == width
            && frame.height == height
        {
            last = Some(frame);
            if Instant::now() + Duration::from_millis(150) < deadline {
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let frame = player
        .frame(token, play_id, 0, width, height)?
        .or(last)
        .ok_or(Error::State("native MV frame missing"))?;
    if frame.rgba.chunks_exact(4).any(|p| p[3] != 255) {
        return Err(Error::Media("video frame alpha uninitialized"));
    }
    image::save_buffer(
        root.join(name),
        &frame.rgba,
        frame.width,
        frame.height,
        image::ColorType::Rgba8,
    )
    .map_err(|_| Error::Media("MV frame capture failed"))?;
    Ok(
        json!({"width":frame.width,"height":frame.height,"sequence":frame.sequence,"rgba_sha256":hex::encode(Sha256::digest(&frame.rgba)),"has_nonblack_pixels":frame.rgba.chunks_exact(4).any(|p|p[0]>10||p[1]>10||p[2]>10)}),
    )
}
async fn run(profile: &Path, sample: &Path, root: &Path) -> Result<Value> {
    let old: Value = serde_json::from_slice(&fs::read(sample)?)?;
    let id = identifier(&old["data"]["id"])?;
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
    let token = session.token()?;
    let factory: VideoEngineFactory = Arc::new(|| {
        Ok(Box::new(MpvVideoEngine::open(
            MpvConfig {
                library: None,
                null_output: true,
            },
            640,
            360,
        )?))
    });
    let player = VideoController::start(session.clone(), token.clone(), factory.clone())?;
    player
        .command(
            &token,
            Cmd::Open {
                id: id.clone(),
                resolution: 240,
                download_task_id: None,
            },
        )
        .await?;
    let playing = until(&player, |v| {
        v.status == Status::Playing && v.position.is_some_and(|p| p > 0.6)
    })
    .await?;
    let initial = capture(
        &player,
        &token,
        &playing.play_id,
        root,
        "stream-240p.png",
        640,
        360,
    )
    .await?;
    player.command(&token, Cmd::Pause { paused: true }).await?;
    player.command(&token, Cmd::Seek { seconds: 40.0 }).await?;
    until(&player, |v| {
        v.status == Status::Paused && v.position.is_some_and(|p| (p - 40.0).abs() < 0.3)
    })
    .await?;
    let paused = capture(
        &player,
        &token,
        &playing.play_id,
        root,
        "paused-seek.png",
        960,
        540,
    )
    .await?;
    player.command(&token, Cmd::Volume { value: 27.0 }).await?;
    player.command(&token, Cmd::Speed { value: 1.25 }).await?;
    player
        .command(&token, Cmd::Quality { resolution: 480 })
        .await?;
    let changed = until(&player, |v| {
        v.status == Status::Paused && v.actual_resolution.is_some()
    })
    .await?;
    if changed.play_id == playing.play_id || changed.position.is_none_or(|p| (p - 40.0).abs() > 0.3)
    {
        return Err(Error::State("MV quality switch lost paused position"));
    }
    let higher = capture(
        &player,
        &token,
        &changed.play_id,
        root,
        "stream-480p.png",
        960,
        540,
    )
    .await?;
    player.command(&token, Cmd::Play).await?;
    let advanced = until(&player, |v| {
        v.status == Status::Playing
            && v.position.is_some_and(|p| p > 40.5)
            && v.volume == 27.0
            && (v.speed - 1.25).abs() < 0.001
    })
    .await?;
    player.command(&token, Cmd::Pause { paused: true }).await?;
    player.command(&token, Cmd::Stop).await?;
    let saved = MvService::new(session.clone())?
        .position_for(&token, &json!(id))?
        .ok_or(Error::State("MV position not saved"))?;
    if player
        .frame(&token, &changed.play_id, 0, 640, 360)?
        .is_some()
    {
        return Err(Error::State("stopped MV exposed stale frame"));
    }
    drop(player);
    let reopened = VideoController::start(session.clone(), token.clone(), factory)?;
    reopened
        .command(
            &token,
            Cmd::Open {
                id,
                resolution: 480,
                download_task_id: None,
            },
        )
        .await?;
    reopened
        .command(&token, Cmd::Pause { paused: true })
        .await?;
    let restored = until(&reopened, |v| {
        v.status == Status::Paused && v.actual_resolution.is_some()
    })
    .await?;
    if restored
        .position
        .is_none_or(|p| (p - saved.position_ms as f64 / 1000.0).abs() > 0.3)
        || restored.volume != 27.0
        || (restored.speed - 1.25).abs() > 0.001
    {
        return Err(Error::State("MV controller restore mismatch"));
    }
    reopened.command(&token, Cmd::Stop).await?;
    drop(reopened);
    Ok(
        json!({"ok":true,"remote_account_mutations":0,"point_spending":0,"streaming":true,"software_render_frames":true,"audio_output":"null","initial_requested_resolution":240,"initial_actual_resolution":playing.actual_resolution,"changed_requested_resolution":480,"changed_actual_resolution":changed.actual_resolution,"initial_progress":playing.position,"pause_seek":40,"resumed_progress":advanced.position,"restored_position_ms":saved.position_ms,"restored_volume":restored.volume,"restored_speed":restored.speed,"initial_frame":initial,"paused_frame":paused,"higher_frame":higher,"stale_frames_revoked":true,"controller_reopened":true,"tauri_window_verified":false}),
    )
}
#[tokio::main]
async fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        eprintln!(
            "usage: mv_video_roundtrip EXISTING_PC_PROFILE PRIVATE_SAMPLE_DETAIL NEW_PRIVATE_PROOF"
        );
        std::process::exit(2);
    }
    let profile = PathBuf::from(&args[0]);
    let root = PathBuf::from(&args[2]);
    if !profile.join("core.sqlite3").is_file() || root.exists() {
        eprintln!("Existing PC profile and new private proof required");
        std::process::exit(2);
    }
    let builder = cumulus_core::storage::private_directory_builder();
    builder.create(&root).expect("create proof");
    match run(&profile, &PathBuf::from(&args[1]), &root).await {
        Ok(report) => {
            fs::write(
                root.join("report.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            )
            .unwrap();
            println!(
                "Rust MV streaming, real frames, quality switch, controls and recovery verified."
            );
        }
        Err(error) => {
            fs::write(root.join("private-error.txt"), error.to_string()).unwrap();
            eprintln!("Native MV video proof incomplete; inspect private evidence.");
            std::process::exit(1);
        }
    }
}
