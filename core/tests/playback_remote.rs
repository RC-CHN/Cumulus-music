#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::{
    Error,
    playback::controller::{PlaybackCommand as Cmd, PlaybackStatus as Status, RemotePlayback},
};
use std::sync::atomic::Ordering;
use support::*;

#[tokio::test]
async fn remote_selection_starts_paused_and_persists_position_without_transient_play() {
    let f = Fixture::new();
    let before = f.command(replace(&[1, 2])).await.unwrap();
    let queue = f.player.queue(&f.token()).await.unwrap();
    f.device.state.lock().unwrap().pauses.clear();
    // Reading the player, ticking its engine and changing volume are not a
    // competing seek/selection and must not invalidate an incoming command.
    f.command(Cmd::Volume(35.0)).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    let result = f
        .command(Cmd::ApplyRemote(RemotePlayback {
            expected: before.fence(),
            entry_id: queue.items[1].entry_id.clone(),
            position: 23.75,
            paused: true,
        }))
        .await
        .unwrap();
    assert_eq!(song(&result), Some(2));
    assert_eq!(result.state, Status::Paused);
    assert_eq!(result.position, Some(23.75));
    assert_eq!(result.volume, 35.0);
    assert_eq!(f.device.state.lock().unwrap().pauses, vec![true]);
    let restarted = f.another_player();
    let restored = restarted.command(&f.token(), Cmd::Restore).await.unwrap();
    assert_eq!(song(&restored), Some(2));
    assert_eq!(restored.position, Some(23.75));
    assert_eq!(restored.state, Status::Paused);
}

#[tokio::test]
async fn newer_local_seek_invalidates_an_incoming_fence_without_touching_the_engine() {
    let f = Fixture::new();
    let before = f.command(replace(&[1, 2])).await.unwrap();
    let entry = before.selected.as_ref().unwrap().entry_id.clone();
    f.command(Cmd::Seek(62.0)).await.unwrap();
    f.device.state.lock().unwrap().pauses.clear();
    let error = f
        .command(Cmd::ApplyRemote(RemotePlayback {
            expected: before.fence(),
            entry_id: entry,
            position: 4.0,
            paused: true,
        }))
        .await
        .err()
        .unwrap();
    assert!(matches!(error.root(), Error::QueueChanged));
    let view = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(view.position, Some(62.0));
    assert_eq!(view.state, Status::Playing);
    assert!(f.device.state.lock().unwrap().pauses.is_empty());
}

#[tokio::test]
async fn local_pause_during_remote_decode_controls_first_presentation_and_newer_revision() {
    let f = Fixture::new();
    let before = f.command(replace(&[1, 2])).await.unwrap();
    f.device.started.notified().await;
    let queue = f.player.queue(&f.token()).await.unwrap();
    f.device.state.lock().unwrap().pauses.clear();
    f.device.hold_load.store(true, Ordering::SeqCst);
    let load = f.start(Cmd::ApplyRemote(RemotePlayback {
        expected: before.fence(),
        entry_id: queue.items[1].entry_id.clone(),
        position: 18.0,
        paused: false,
    }));
    f.device.started.notified().await;
    let local = f.command(Cmd::Pause(true)).await.unwrap();
    f.device.hold_load.store(false, Ordering::SeqCst);
    let result = load.await.unwrap().unwrap();
    assert_eq!(result.control_revision, local.control_revision);
    assert!(result.control_revision > before.control_revision + 1);
    assert_eq!(result.state, Status::Paused);
    assert_eq!(result.position, Some(18.0));
    assert_eq!(f.device.state.lock().unwrap().pauses, vec![true]);
}

#[tokio::test]
async fn remote_apply_respects_trial_coordinates_and_stops_after_partial_engine_failure() {
    let f = Fixture::new();
    f.api.trial.store(true, Ordering::SeqCst);
    let before = f.command(replace(&[1])).await.unwrap();
    let entry = before.selected.as_ref().unwrap().entry_id.clone();
    assert!(
        f.command(Cmd::ApplyRemote(RemotePlayback {
            expected: before.fence(),
            entry_id: entry.clone(),
            position: 20.0,
            paused: true,
        }))
        .await
        .is_err()
    );
    let after = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(after.position, Some(30.0));
    assert_eq!(after.state, Status::Playing);
    let valid = f
        .command(Cmd::ApplyRemote(RemotePlayback {
            expected: after.fence(),
            entry_id: entry.clone(),
            position: 35.0,
            paused: true,
        }))
        .await
        .unwrap();
    assert_eq!(valid.position, Some(35.0));
    assert_eq!(f.device.state.lock().unwrap().view.position, Some(5.0));
    f.device.fail_seek.store(true, Ordering::SeqCst);
    assert!(
        f.command(Cmd::ApplyRemote(RemotePlayback {
            expected: valid.fence(),
            entry_id: entry,
            position: 38.0,
            paused: false,
        }))
        .await
        .is_err()
    );
    let failed = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(failed.state, Status::Error);
    assert!(f.device.state.lock().unwrap().view.paused);
    assert!(f.device.state.lock().unwrap().view.entry_id.is_none());
}
