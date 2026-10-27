//! Real account -> authorized CDN -> native decoder/output -> persistent actor.
//! Explicit private PC profile, isolated state, zero volume, no remote writes.
use cumulus_core::{
    Error, Result,
    account::{AccountApi, AccountSession, credentials::Credentials},
    library::LocalLibrary,
    membership::Quality,
    playback::{
        PlaybackMode,
        controller::{
            PlaybackCommand as Cmd, PlaybackController, PlaybackSnapshot, PlaybackStatus,
        },
        engine::{MpvConfig, MpvEngine},
        source::SourceResolver,
    },
    protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};

async fn wait(
    player: &PlaybackController,
    predicate: impl Fn(&PlaybackSnapshot) -> bool,
) -> Result<PlaybackSnapshot> {
    let mut receiver = player.subscribe();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let value = receiver.borrow().clone();
            if value.state == PlaybackStatus::Error {
                return Err(Error::State("playback entered error state"));
            }
            if predicate(&value) {
                return Ok(value);
            }
            receiver
                .changed()
                .await
                .map_err(|_| Error::State("playback closed"))?;
        }
    })
    .await
    .map_err(|_| Error::State("playback proof timed out"))?
}
async fn run() -> Result<Value> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(Error::InvalidInput(
            "usage: playback_roundtrip EXISTING_PC_PROFILE NEW_STATE_DIRECTORY LOCAL_AUDIO",
        ));
    }
    let profile = PathBuf::from(&args[0]);
    if !profile.join("core.sqlite3").is_file() {
        return Err(Error::InvalidInput("existing private profile required"));
    }
    let directory = PathBuf::from(&args[1]);
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&directory)?;
    let local = PathBuf::from(&args[2]).canonicalize()?;
    let store = Arc::new(StateStore::open(&directory)?);
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let api = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
    let session = Arc::new(AccountSession::new(api, Some(store.clone())));
    session.refresh().await?;
    let token = session.token()?;
    let ids = session
        .run_for(&token, |api, uid| async move {
            AccountApi::new(api).liked_ids(&json!(uid)).await
        })
        .await?;
    if ids.len() < 2 {
        return Err(Error::State("two account tracks required"));
    }
    let library = Arc::new(LocalLibrary::new(store.clone())?);
    let resolver = Arc::new(SourceResolver::new(session.clone(), Some(library)));
    let engine = tokio::task::spawn_blocking(|| MpvEngine::open(MpvConfig::default()))
        .await
        .map_err(|_| Error::State("engine creation failed"))??;
    let player = PlaybackController::start(
        session.clone(),
        store.clone(),
        resolver.clone(),
        Box::new(engine),
    )?;
    player.command(&token, Cmd::Volume(0.0)).await?;
    player
        .command(&token, Cmd::Mode(PlaybackMode::Sequential))
        .await?;
    let resources = ids[..2]
        .iter()
        .map(|id| json!({"id":id,"level":"lossless"}))
        .collect();
    let first = player
        .command(
            &token,
            Cmd::Replace {
                resources,
                index: 0,
                source: json!({"kind":"isolated-proof"}),
            },
        )
        .await?;
    assert_eq!(first.state, PlaybackStatus::Playing);
    wait(&player, |s| s.position.is_some_and(|p| p > 0.3)).await?;
    let paused = player.command(&token, Cmd::Pause(true)).await?;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let stable = player.command(&token, Cmd::Snapshot).await?;
    assert!((paused.position.unwrap() - stable.position.unwrap()).abs() < 0.08);
    player.command(&token, Cmd::Seek(10.0)).await?;
    player.command(&token, Cmd::Speed(1.25)).await?;
    let quality = player
        .command(&token, Cmd::Quality(Quality::Standard))
        .await?;
    assert_eq!(quality.state, PlaybackStatus::Paused);
    assert!((quality.position.unwrap() - 10.0).abs() < 0.08);
    let refresh = player.command(&token, Cmd::RefreshSource).await?;
    assert_eq!(refresh.state, PlaybackStatus::Paused);
    player.command(&token, Cmd::Play).await?;
    wait(&player, |s| s.position.is_some_and(|p| p > 10.3)).await?;
    let next = player.command(&token, Cmd::Next).await?;
    assert_ne!(
        next.selected.as_ref().map(|i| &i.entry_id),
        first.selected.as_ref().map(|i| &i.entry_id)
    );
    wait(&player, |s| s.position.is_some_and(|p| p > 0.2)).await?;
    player.command(&token, Cmd::Pause(true)).await?;
    player.command(&token, Cmd::Seek(15.0)).await?;
    player.command(&token, Cmd::Stop).await?;
    let mut closed = player.subscribe();
    drop(player);
    tokio::time::timeout(Duration::from_secs(5), async {
        while closed.changed().await.is_ok() {}
    })
    .await
    .map_err(|_| Error::State("old player did not close"))?;
    let engine = tokio::task::spawn_blocking(|| MpvEngine::open(MpvConfig::default()))
        .await
        .map_err(|_| Error::State("engine recreation failed"))??;
    let player = PlaybackController::start(session, store, resolver, Box::new(engine))?;
    let restored = player.command(&token, Cmd::Restore).await?;
    assert_eq!(restored.state, PlaybackStatus::Paused);
    assert_eq!(restored.position, Some(15.0));
    assert_eq!(restored.volume, 0.0);
    assert_eq!(restored.speed, 1.25);
    let local_view = player
        .command(
            &token,
            Cmd::Replace {
                resources: vec![json!({"path":local})],
                index: 0,
                source: json!({"kind":"local-proof"}),
            },
        )
        .await?;
    assert_eq!(local_view.state, PlaybackStatus::Playing);
    wait(&player, |s| s.position.is_some_and(|p| p > 0.3)).await?;
    player.command(&token, Cmd::Stop).await?;
    Ok(
        json!({"schema":1,"ok":true,"implementation":"rust-core-native-player","online_tracks":2,
        "requested_output":"system_muted","initial_quality":first.actual_quality,"changed_quality":quality.actual_quality,
        "progress":true,"pause_stable":true,"seek":true,"speed":true,"quality_preserves_position":true,
        "refresh_preserves_pause":true,"next":true,"reopened_paused":true,"restored_position":15,
        "local_media_played":true,"local_kind":local.extension().and_then(|e|e.to_str()),"remote_mutations":0}),
    )
}
#[tokio::main]
async fn main() {
    match run().await {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
