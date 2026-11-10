#[path = "support/together_fixture.rs"]
mod fixture;
#[path = "support/together_incoming.rs"]
mod incoming_tests;
#[path = "support/together_lifecycle_host.rs"]
mod lifecycle_tests;
#[path = "support/together_mini.rs"]
mod mini_tests;
#[path = "support/together_progress.rs"]
mod progress_tests;
#[path = "support/together_queue_clear.rs"]
mod queue_clear_tests;
#[path = "support/together_queue_loss.rs"]
mod queue_loss_tests;
#[path = "support/together_shared.rs"]
mod shared_tests;
use cumulus_core::{
    Error,
    account::AccountSession,
    client::{MusicClient, dto::*},
    playback::PlaybackMode,
};
use fixture::{Backend, engine, im, world};
use serde_json::{Value, json};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

#[tokio::test]
async fn message_connection_reconnects_without_replaying_playback_and_exit_awaits_cleanup() {
    use cumulus_core::listen_together::transport::ConnectionState;
    use std::sync::atomic::Ordering;
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (session, client) = world(root.path(), api.clone());
    let device = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(device.clone())));
    play(&client).await;
    client
        .together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    let ready = wait(&client, |v| {
        v.connection.state == ConnectionState::Connected
            && v.connection.received == 1
            && v.tracks.len() == 3
            && v.target_id.is_some()
    })
    .await;
    let id = ready.room.unwrap().id;
    let write_count = || {
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.ends_with("command/report"))
            .count()
    };
    let before = write_count();
    client
        .together_command(
            "1",
            TogetherCommand::Reconnect {
                room_id: id.clone(),
            },
        )
        .await
        .unwrap();
    wait(&client, |v| {
        v.connection.state == ConnectionState::Connected && v.connection.received == 1
    })
    .await;
    assert_eq!(device.peak.load(Ordering::SeqCst), 1);
    assert_eq!(device.calls.load(Ordering::SeqCst), 2);
    assert_eq!(device.closed.load(Ordering::SeqCst), 1);
    assert_eq!(
        write_count(),
        before,
        "reconnect must not replay room/player writes"
    );
    let body = serde_json::to_string(&client.together_view("1", false).await.unwrap()).unwrap();
    assert!(
        !body.contains("private-token")
            && !body.contains("test-account")
            && !body.contains("never serialize")
    );
    client
        .together_command("1", TogetherCommand::End { room_id: id })
        .await
        .unwrap();
    assert_eq!(device.active.load(Ordering::SeqCst), 0);
    assert_eq!(device.closed.load(Ordering::SeqCst), 2);
    assert!(!client.has_together_cleanup().unwrap());
    assert_eq!(session.token().unwrap().generation, 1);
}

#[tokio::test]
async fn failed_native_cleanup_remains_visible_and_cannot_be_claimed_clean_on_second_close() {
    use cumulus_core::listen_together::transport::ConnectionState;
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    let device = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(device.clone())));
    play(&client).await;
    client
        .together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    let ready = wait(&client, |v| {
        v.connection.state == ConnectionState::Connected && v.tracks.len() == 3
    })
    .await;
    device.close_failure.store(true, Ordering::SeqCst);
    assert!(
        client
            .together_command(
                "1",
                TogetherCommand::End {
                    room_id: ready.room.unwrap().id
                }
            )
            .await
            .is_err()
    );
    let view = client.together_view("1", false).await.unwrap().value;
    assert!(view.room.is_none() && view.local_recovery);
    assert_eq!(view.connection.state, ConnectionState::Failed);
    assert!(client.close_together().await.is_err());
    assert!(client.has_together_cleanup().unwrap());
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.ends_with("end/v2"))
            .count(),
        1
    );
}

async fn wait(client: &MusicClient, predicate: impl Fn(&TogetherView) -> bool) -> TogetherView {
    let mut last = None;
    let result = tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let v = client.together_view("1", false).await.unwrap().value;
            if predicate(&v) {
                return v;
            }
            last = Some(v);
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await;
    result.unwrap_or_else(|_| {
        panic!(
            "together native state did not settle; last public view: {}",
            serde_json::to_string(&last).unwrap()
        )
    })
}
async fn play(client: &MusicClient) {
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["91".into(), "92".into(), "93".into()],
                index: 0,
                quality: cumulus_core::membership::Quality::Standard,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn actual_native_player_drives_room_queue_seek_pause_next_remove_and_restores_preferences() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_s, c) = world(root.path(), api.clone());
    play(&c).await;
    c.player_action("1", PlayerAction::Speed { value: 1.25 })
        .await
        .unwrap();
    c.player_action(
        "1",
        PlayerAction::Mode {
            mode: PlaybackMode::Sequential,
        },
    )
    .await
    .unwrap();
    c.together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    let v = wait(&c, |v| {
        v.tracks.len() == 3 && v.pending.is_empty() && v.target_id.as_deref() == Some("91")
    })
    .await;
    assert!(v.attached);
    assert!(matches!(
        c.player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Sequential
            }
        )
        .await,
        Err(Error::State(
            "together does not support sequential playback"
        ))
    ));
    assert!(!v.realtime_ready);
    assert!(c.has_together_cleanup().unwrap());
    assert!(matches!(
        c.begin_qr().await,
        Err(Error::State("leave together before switching accounts"))
    ));
    assert_eq!(v.tracks[1].name, "Song \"92\"");
    assert!(!serde_json::to_string(&v).unwrap().contains("SECRET"));
    assert!(
        !serde_json::to_string(&v)
            .unwrap()
            .contains("private-channel")
    );
    assert!(
        c.player_action("1", PlayerAction::Speed { value: 1.5 })
            .await
            .is_err()
    );
    c.player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    wait(&c, |v| !v.playing && v.pending.is_empty()).await;
    c.player_action("1", PlayerAction::Seek { seconds: 12.0 })
        .await
        .unwrap();
    wait(&c, |v| v.progress_ms == Some(12000) && v.pending.is_empty()).await;
    c.player_action("1", PlayerAction::Next).await.unwrap();
    wait(&c, |v| {
        v.target_id.as_deref() == Some("92") && v.pending.is_empty()
    })
    .await;
    c.together_command(
        "1",
        TogetherCommand::Remove {
            room_id: "test-room".into(),
            song_id: "93".into(),
        },
    )
    .await
    .unwrap();
    wait(&c, |v| v.tracks.len() == 2 && v.pending.is_empty()).await;
    c.together_command(
        "1",
        TogetherCommand::End {
            room_id: "test-room".into(),
        },
    )
    .await
    .unwrap();
    let mut subscription = c.subscribe_playback("1").await.unwrap();
    let current = subscription.current().unwrap();
    assert_eq!(current.speed, 1.25);
    assert_eq!(current.mode, PlaybackMode::Sequential);
    assert_eq!(c.player_queue("1").await.unwrap().tracks.len(), 2);
    assert!(
        c.together_view("1", false)
            .await
            .unwrap()
            .value
            .room
            .is_none()
    );
    assert!(!api.state.lock().unwrap().inside);
    assert!(!c.has_together_cleanup().unwrap());
    let _ = &mut subscription;
    let commands = api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(p, _)| p.ends_with("play/command/report"))
        .map(|(_, v)| serde_json::from_str::<Value>(v["commandInfo"].as_str().unwrap()).unwrap())
        .collect::<Vec<_>>();
    assert!(commands.iter().any(|v| v["commandType"] == "NEXT"));
    assert!(
        commands
            .iter()
            .any(|v| v["commandType"] == "PROGRESS" && v["progress"] == 12000)
    );
}
#[tokio::test]
async fn lost_report_blocks_resending_but_native_audio_remains_controllable_and_can_resume() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_s, c) = world(root.path(), api.clone());
    play(&c).await;
    c.together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    wait(&c, |v| {
        v.tracks.len() == 3 && v.target_id.is_some() && v.pending.is_empty()
    })
    .await;
    api.drop_play.store(true, Ordering::SeqCst);
    c.player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let v = wait(&c, |v| !v.pending.is_empty() && v.sync_error.is_some()).await;
    assert_eq!(v.pending.len(), 1);
    let before = api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(p, _)| p.ends_with("play/command/report"))
        .count();
    c.player_action("1", PlayerAction::Seek { seconds: 15.0 })
        .await
        .unwrap();
    c.player_action("1", PlayerAction::Play).await.unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        before,
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.ends_with("play/command/report"))
            .count()
    );
    c.together_command(
        "1",
        TogetherCommand::Reconcile {
            operation_id: v.pending[0].id.clone(),
        },
    )
    .await
    .unwrap();
    c.together_command(
        "1",
        TogetherCommand::Publish {
            room_id: "test-room".into(),
        },
    )
    .await
    .unwrap();
    wait(&c, |v| {
        v.playing && v.progress_ms == Some(15000) && v.pending.is_empty() && v.sync_error.is_none()
    })
    .await;
    // Local selection may change while room writes are suspended. Resuming
    // must select that song remotely, not send a cross-song PROGRESS command.
    api.drop_play.store(true, Ordering::SeqCst);
    c.player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let pending = wait(&c, |v| !v.pending.is_empty() && v.sync_error.is_some()).await;
    c.player_action("1", PlayerAction::Next).await.unwrap();
    let subscription = c.subscribe_playback("1").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = subscription.current().unwrap();
            if snapshot.state == cumulus_core::playback::controller::PlaybackStatus::Playing
                && snapshot.track.as_ref().is_some_and(|v| v.id == "92")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    c.together_command(
        "1",
        TogetherCommand::Reconcile {
            operation_id: pending.pending[0].id.clone(),
        },
    )
    .await
    .unwrap();
    c.together_command(
        "1",
        TogetherCommand::Publish {
            room_id: "test-room".into(),
        },
    )
    .await
    .unwrap();
    wait(&c, |v| {
        v.target_id.as_deref() == Some("92") && v.playing && v.pending.is_empty()
    })
    .await;
    assert_eq!(
        api.state.lock().unwrap().play.as_ref().unwrap()["commandType"],
        "GOTO"
    );
    c.close_together().await.unwrap();
    assert!(!api.state.lock().unwrap().inside);
}
#[tokio::test]
async fn explicit_seek_to_zero_while_playing_is_reported_once() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, c) = world(root.path(), api.clone());
    play(&c).await;
    c.together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    wait(&c, |v| v.target_id.is_some() && v.pending.is_empty()).await;
    c.player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    wait(&c, |v| !v.playing && v.pending.is_empty()).await;
    c.player_action("1", PlayerAction::Seek { seconds: 12.0 })
        .await
        .unwrap();
    wait(&c, |v| v.progress_ms == Some(12000) && v.pending.is_empty()).await;
    c.player_action("1", PlayerAction::Play).await.unwrap();
    wait(&c, |v| v.playing && v.pending.is_empty()).await;
    c.player_action("1", PlayerAction::Seek { seconds: 0.0 })
        .await
        .unwrap();
    wait(&c, |v| {
        v.playing && v.progress_ms == Some(0) && v.pending.is_empty()
    })
    .await;
    let reports = api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(p, _)| p.ends_with("play/command/report"))
        .map(|(_, v)| serde_json::from_str::<Value>(v["commandInfo"].as_str().unwrap()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        reports
            .iter()
            .filter(|v| v["commandType"] == "GOTO" && v["progress"] == 0)
            .count(),
        1
    );
    c.close_together().await.unwrap();
}
#[tokio::test]
async fn reopening_requires_explicit_attach_and_account_switch_never_exposes_old_room() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (s, c) = world(root.path(), api.clone());
    play(&c).await;
    c.together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    wait(&c, |v| v.target_id.is_some() && v.pending.is_empty()).await;
    drop(c);
    drop(s);
    let (s, c) = world(root.path(), api.clone());
    let before = api.calls.lock().unwrap().len();
    assert!(c.has_together_cleanup().unwrap());
    let view = c.together_view("1", false).await.unwrap().value;
    assert!(view.room.is_some());
    assert!(!view.attached);
    assert_eq!(before, api.calls.lock().unwrap().len());
    c.together_command(
        "1",
        TogetherCommand::Attach {
            room_id: "test-room".into(),
        },
    )
    .await
    .unwrap();
    assert!(c.together_view("1", false).await.unwrap().value.attached);
    s.adopt(api.clone(), json!({"userId":2}), &s.token().unwrap())
        .unwrap();
    let new = c.together_view("2", false).await.unwrap().value;
    assert!(new.room.is_none());
    assert!(!new.attached);
    assert!(new.pending.is_empty());
    assert!(new.sync_error.is_none());
    assert!(matches!(
        c.together_command("1", TogetherCommand::Create).await,
        Err(Error::StaleResponse)
    ));
}
