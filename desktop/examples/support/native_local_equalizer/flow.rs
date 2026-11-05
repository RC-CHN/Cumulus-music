//! Ordinary player controls; no direct construction of a held reader or graph.
use super::engine::Evidence;
use cumulus_core::{
    Error, Result,
    account::SessionStatus,
    client::{
        MusicClient,
        dto::{PlayerAction, PlayerResourceKind, PlayerView},
    },
    effects::equalizer::{EqualizerPreferences, EqualizerSettings, EqualizerStatus},
    playback::{PlaybackMode, controller::PlaybackStatus},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

const SOURCE_DURATION: f64 = 88_207.0 / 44_100.0;

fn require(condition: bool, message: &'static str) -> Result<()> {
    if !condition {
        return Err(Error::State(message));
    }
    Ok(())
}

async fn until(
    client: &MusicClient,
    epoch: &str,
    stage: &'static str,
    observe: impl Fn(&PlayerView) -> bool,
) -> Result<PlayerView> {
    let result = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let view = client.player_snapshot(epoch).await?;
            if view.error.is_some() || view.state == PlaybackStatus::Error {
                eprintln!(
                    "ordinary local EQ actor failed at {stage}: {view:?}",
                    view = serde_json::to_value(&view)?
                );
                return Err(Error::State("ordinary local EQ playback failed"));
            }
            if observe(&view) {
                return Ok(view);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await;
    match result {
        Ok(result) => result,
        Err(_) => {
            eprintln!("ordinary local EQ proof timed out at {stage}");
            Err(Error::State(stage))
        }
    }
}

fn same_play(value: &PlayerView, expected: &PlayerView) -> bool {
    value.entry_id == expected.entry_id
        && value.play_id == expected.play_id
        && value.queue_revision == expected.queue_revision
        && value.queue_len == 1
}

fn curve(gain: f32) -> Result<EqualizerSettings> {
    let mut gains = [0.0; 10];
    gains[3] = gain;
    EqualizerSettings::new(true, gains)?.with_tone(4, -2)
}

async fn configure(
    client: &MusicClient,
    epoch: &str,
    previous: &PlayerView,
    settings: EqualizerSettings,
) -> Result<PlayerView> {
    let value = client
        .player_action(
            epoch,
            PlayerAction::Equalizer {
                expected_revision: previous.equalizer.revision.clone(),
                settings,
                preset: None,
            },
        )
        .await?;
    require(
        value.equalizer.settings == settings
            && value.equalizer.revision != previous.equalizer.revision,
        "ordinary EQ did not commit the exact setting",
    )?;
    Ok(value)
}

pub async fn verify(
    client: &MusicClient,
    store: &StateStore,
    path: PathBuf,
    evidence: &Evidence,
    expect_graph: bool,
) -> Result<Value> {
    let account = client.account()?;
    require(
        account.status != SessionStatus::Authenticated,
        "fixture must stay a local guest",
    )?;
    let epoch = account.epoch;
    let initial = client
        .player_action(
            &epoch,
            PlayerAction::Mode {
                mode: PlaybackMode::Sequential,
            },
        )
        .await?;
    configure(client, &epoch, &initial, curve(3.0)?).await?;
    eprintln!("ordinary local EQ proof: settings_committed");
    let selected = client.play_native_open_files(&epoch, vec![path]).await?;
    require(
        selected.resource_kind == Some(PlayerResourceKind::Local)
            && selected.entry_id.is_some()
            && selected.play_id.is_some()
            && selected.mode == PlaybackMode::Sequential,
        "native open did not select a single ordinary local entry",
    )?;
    let playing = until(client, &epoch, "first_position", |view| {
        same_play(view, &selected)
            && view.state == PlaybackStatus::Playing
            && view.position.is_some_and(|pos| pos >= 0.10)
            && (!expect_graph || view.equalizer.status == EqualizerStatus::Processing)
    })
    .await?;
    eprintln!("ordinary local EQ proof: first_native_position");
    let play_id = playing.play_id.as_ref().expect("selected playback ID");
    let pause = |paused| PlayerAction::PauseFor {
        play_id: play_id.clone(),
        expected_queue_revision: playing.queue_revision.clone(),
        paused,
    };
    client.player_action(&epoch, pause(true)).await?;
    let paused = until(client, &epoch, "pause", |view| {
        same_play(view, &playing) && view.state == PlaybackStatus::Paused && !view.native_seeking
    })
    .await?;
    tokio::time::sleep(Duration::from_millis(175)).await;
    let held = client.player_snapshot(&epoch).await?;
    require(
        same_play(&held, &paused)
            && held.state == PlaybackStatus::Paused
            && held
                .position
                .zip(paused.position)
                .is_some_and(|(a, b)| (a - b).abs() < 0.06),
        "paused clock did not hold",
    )?;
    eprintln!("ordinary local EQ proof: paused_clock_held");

    let settings = curve(6.0)?;
    let edited = configure(client, &epoch, &held, settings).await?;
    require(
        same_play(&edited, &held),
        "EQ edit replaced the logical play",
    )?;
    client
        .player_action(
            &epoch,
            PlayerAction::SeekFor {
                play_id: play_id.clone(),
                expected_queue_revision: playing.queue_revision.clone(),
                seconds: 0.5,
            },
        )
        .await?;
    let sought = until(client, &epoch, "paused_seek", |view| {
        same_play(view, &playing)
            && view.state == PlaybackStatus::Paused
            && !view.native_seeking
            && view.position.is_some_and(|p| (p - 0.5).abs() < 0.06)
    })
    .await?;
    eprintln!("ordinary local EQ proof: paused_seek_verified");
    client.player_action(&epoch, pause(false)).await?;
    let resumed = until(client, &epoch, "resume_position", |view| {
        same_play(view, &sought)
            && view.state == PlaybackStatus::Playing
            && view.position.is_some_and(|p| p > 0.65)
            && (!expect_graph || view.equalizer.status == EqualizerStatus::Processing)
    })
    .await?;
    let ended = until(client, &epoch, "native_eof", |view| {
        same_play(view, &playing) && view.state == PlaybackStatus::Ended
    })
    .await?;
    require(
        ended
            .position
            .is_some_and(|p| (p - SOURCE_DURATION).abs() < 0.02),
        "ordinary actor end did not use the source timeline",
    )?;
    eprintln!("ordinary local EQ proof: native_eof");
    let saved = EqualizerPreferences::read(store, "local-guest")?;
    require(
        saved.settings == settings
            && saved.revision.to_string() == edited.equalizer.revision
            && ended.equalizer.revision == edited.equalizer.revision,
        "seek or EOF altered the durable EQ setting",
    )?;
    let trace = evidence.lock().unwrap();
    let ([load], [seek]) = (trace.loads.as_slice(), trace.seeks.as_slice()) else {
        return Err(Error::State("expected one real engine load and one seek"));
    };
    require(
        trace.ordinary_loads == 0
            && load.native_file
            && load.start == 0.0
            && seek.old_entry == load.entry
            && seek.seconds == 0.5,
        "ordinary actor did not attempt the held-file EQ boundary",
    )?;
    let final_entry = if expect_graph {
        let replacement = seek
            .replacement
            .ok_or(Error::State("missing native seek replacement"))?;
        require(
            load.graph_entry == Some(load.entry)
                && seek.graph_entry == Some(replacement)
                && replacement != load.entry
                && trace.processed_entries.contains(&load.entry)
                && trace.processed_entries.contains(&replacement)
                && trace.local_updates.contains(&settings),
            "new graph did not own both native entries or apply the live edit",
        )?;
        replacement
    } else {
        require(
            load.graph_entry.is_none()
                && seek.replacement.is_none()
                && seek.graph_entry.is_none()
                && trace.processed_entries.is_empty()
                && trace.local_updates.is_empty(),
            "wrong digest reached the optional local EQ graph",
        )?;
        load.entry
    };
    require(
        trace.eof_entries == [final_entry],
        "unexpected native EOF receipt",
    )?;
    require(
        client.account()?.status != SessionStatus::Authenticated,
        "native open changed the account scope",
    )?;
    Ok(json!({
        "ok": true, "kind": "ordinary_native_local_equalizer_v1",
        "localGuest": true, "actualActor": true, "explicitNativeFile": true, "audioOutput": "null",
        "wrongDigestStockFallback": !expect_graph, "fixedGraphSelected": expect_graph,
        "sameLogicalPlayAcrossSeek": true, "durableEqualizerRevision": edited.equalizer.revision,
        "durableEqualizerSettings": saved.settings,
        "positions": {"first":playing.position,"held":held.position,"sought":sought.position,
                      "resumed":resumed.position,"ended":ended.position},
        "native": &*trace,
    }))
}
