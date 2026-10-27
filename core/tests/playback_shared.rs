#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::{
    Error,
    playback::{
        PlaybackConstraint, PlaybackMode,
        controller::{
            PlaybackCommand as Cmd, PlaybackStatus as Status, QueueTransport, QueueUpdate,
        },
        engine::EndReason,
    },
};
use serde_json::json;
use std::sync::atomic::Ordering;
use support::*;

#[tokio::test]
async fn queue_install_preserves_native_entry_and_exact_random_order_then_skips_blocked() {
    let f = Fixture::new();
    let before = f.command(replace(&[1, 2, 3])).await.unwrap();
    let mut queue = f.player.queue(&f.token()).await.unwrap();
    queue.items[1].resource["playback_constraint"] = json!(PlaybackConstraint::Blocked);
    queue.mode = PlaybackMode::Random;
    queue.order = vec![
        queue.items[2].entry_id.clone(),
        queue.items[0].entry_id.clone(),
        queue.items[1].entry_id.clone(),
    ];
    let expected = queue.clone();
    let state = f
        .command(Cmd::UpdateQueue(QueueUpdate {
            expected: before.fence(),
            queue,
            transport: None,
        }))
        .await
        .unwrap();
    assert_eq!(state.play_id, before.play_id);
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    assert_eq!(f.player.queue(&f.token()).await.unwrap(), expected);
    assert_eq!(song(&f.command(Cmd::Next).await.unwrap()), Some(3));
    let blocked = f
        .command(Cmd::Select(expected.items[1].entry_id.clone()))
        .await
        .err()
        .unwrap();
    assert!(matches!(
        blocked.root(),
        Error::State("resource is not jointly playable")
    ));
    assert_eq!(song(&f.command(Cmd::Snapshot).await.unwrap()), Some(3));
}

#[tokio::test]
async fn common_preview_restricts_full_audio_and_clip_audio_in_their_actual_media_clocks() {
    for clip in [false, true] {
        let f = Fixture::new();
        f.api.trial.store(clip, Ordering::SeqCst);
        let before = f.command(replace(&[1, 2])).await.unwrap();
        let mut queue = f.player.queue(&f.token()).await.unwrap();
        queue.items[0].resource["playback_constraint"] = json!(PlaybackConstraint::Preview {
            start: 40.0,
            end: 50.0
        });
        queue.mode = PlaybackMode::SingleLoop;
        let state = f
            .command(Cmd::UpdateQueue(QueueUpdate {
                expected: before.fence(),
                queue,
                transport: Some(QueueTransport {
                    position: Some(0.0),
                    paused: true,
                }),
            }))
            .await
            .unwrap();
        assert_eq!(state.position, Some(40.0));
        assert_eq!(state.state, Status::Paused);
        assert_eq!(
            f.device.state.lock().unwrap().bounds.last(),
            Some(&(
                if clip { 10.0 } else { 40.0 },
                Some(if clip { 20.0 } else { 50.0 })
            ))
        );
        f.command(Cmd::Seek(45.0)).await.unwrap();
        assert_eq!(
            f.device.state.lock().unwrap().view.position,
            Some(if clip { 15.0 } else { 45.0 })
        );
        assert!(f.command(Cmd::Seek(52.0)).await.is_err());
        f.command(Cmd::Play).await.unwrap();
        let native = f.device.state.lock().unwrap().view.entry_id.unwrap();
        event(&f.device, native, EndReason::Eof);
        let after = until(&f.player, |s| {
            song(s) == Some(2) && s.state == Status::Playing
        })
        .await;
        assert_eq!(
            song(&after),
            Some(2),
            "shared trial must advance even in single-loop mode"
        );
    }
}

#[tokio::test]
async fn stale_shared_queue_cannot_replace_local_selection_and_empty_shared_access_stops() {
    let f = Fixture::new();
    let before = f.command(replace(&[1, 2])).await.unwrap();
    let queue = f.player.queue(&f.token()).await.unwrap();
    let current = f.command(Cmd::Next).await.unwrap();
    assert!(matches!(
        f.command(Cmd::UpdateQueue(QueueUpdate {
            expected: before.fence(),
            queue: queue.clone(),
            transport: None
        }))
        .await
        .err()
        .unwrap()
        .root(),
        Error::QueueChanged
    ));
    assert_eq!(song(&f.player.snapshot()), Some(2));
    let mut blocked = queue;
    for entry in &mut blocked.items {
        entry.resource["playback_constraint"] = json!(PlaybackConstraint::Blocked);
    }
    blocked.current = None;
    let stopped = f
        .command(Cmd::UpdateQueue(QueueUpdate {
            expected: current.fence(),
            queue: blocked,
            transport: None,
        }))
        .await
        .unwrap();
    assert_eq!(stopped.state, Status::Idle);
    assert!(f.device.state.lock().unwrap().view.entry_id.is_none());
    assert_eq!(stopped.queue_len, 2);
}
