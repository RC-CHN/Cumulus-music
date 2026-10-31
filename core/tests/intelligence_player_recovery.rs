#[allow(dead_code)]
#[path = "support/intelligence_player.rs"]
mod support;
use cumulus_core::{
    Error,
    playback::{
        PlaybackMode, PlaybackQueue,
        controller::{IntelligenceUpdate, PlaybackCommand as Cmd, PlaybackStatus},
        intelligence::{IntelligenceAction, IntelligenceFailure, IntelligencePhase},
    },
};
use serde_json::json;
use std::{sync::atomic::Ordering, time::Duration};
use support::*;

#[tokio::test]
async fn account_reopen_preserves_both_phases_without_fetching_recommendations_or_autoplaying() {
    let f = Fixture::new();
    f.command(replace(&[7, 8, 9])).await.unwrap();
    let seeded = act(&f, begin(8, true)).await.unwrap();
    let id = seeded.intelligence.unwrap().id;
    let reopened = f.another_player();
    let empty = reopened.command(&f.token(), Cmd::Snapshot).await.unwrap();
    assert_eq!(empty.state, PlaybackStatus::Idle);
    assert_eq!(empty.intelligence.unwrap().phase, IntelligencePhase::Seeded);
    assert!(empty.play_id.is_none());
    drop(reopened);
    act(&f, complete(&id, &[90, 8, 91])).await.unwrap();
    f.command(Cmd::Pause(true)).await.unwrap();
    let before = f.command(Cmd::Seek(21.0)).await.unwrap();
    let expected = f.player.queue(&f.token()).await.unwrap();
    let reopened = f.another_player();
    let idle = reopened.command(&f.token(), Cmd::Snapshot).await.unwrap();
    assert_eq!(idle.intelligence.unwrap().phase, IntelligencePhase::Active);
    assert_eq!(idle.state, PlaybackStatus::Idle);
    let restored = reopened.command(&f.token(), Cmd::Restore).await.unwrap();
    assert_eq!(restored.state, PlaybackStatus::Paused);
    assert_eq!(restored.position, Some(21.0));
    assert_eq!(restored.selected, before.selected);
    assert_eq!(reopened.queue(&f.token()).await.unwrap(), expected);
    let loaded = PlaybackQueue::new(Some(f.store.clone()), "9").unwrap();
    assert_eq!(loaded.intelligence_session().unwrap().context.id, id);
    assert_eq!(
        ids(&loaded.intelligence_session().unwrap().backup),
        vec![json!(7), json!(8), json!(9)]
    );
    assert!(
        PlaybackQueue::new(Some(f.store.clone()), "10")
            .unwrap()
            .intelligence_session()
            .is_none()
    );
}

#[tokio::test]
async fn delayed_reply_checks_both_the_current_fence_and_the_session_instead_of_replacing_newer_work()
 {
    let f = Fixture::new();
    let seeded = act(&f, begin(7, false)).await.unwrap();
    let id = seeded.intelligence.as_ref().unwrap().id.clone();
    let stale = seeded.fence();
    f.command(Cmd::Pause(true)).await.unwrap();
    let denied = f
        .command(Cmd::Intelligence(IntelligenceUpdate {
            expected: stale,
            action: complete(&id, &[90]),
        }))
        .await;
    assert!(matches!(
        root(denied.as_ref().err().unwrap()),
        Error::QueueChanged
    ));
    assert_eq!(f.player.queue(&f.token()).await.unwrap().items.len(), 1);
    act(&f, complete(&id, &[90])).await.unwrap();
    f.command(Cmd::Mode(PlaybackMode::Sequential))
        .await
        .unwrap();
    let current = f.player.queue(&f.token()).await.unwrap();
    let result = act(
        &f,
        IntelligenceAction::Fail {
            session_id: id.clone(),
            failure: IntelligenceFailure::RequestFailed,
        },
    )
    .await;
    assert!(matches!(
        root(result.as_ref().err().unwrap()),
        Error::StaleResponse
    ));
    assert!(act(&f, complete(&id, &[99])).await.is_err());
    assert_eq!(f.player.queue(&f.token()).await.unwrap(), current);
}

#[tokio::test]
async fn begin_and_complete_roll_back_queue_and_session_together_on_persistence_failure() {
    let f = Fixture::new();
    f.command(replace(&[7, 8, 9])).await.unwrap();
    f.command(Cmd::Pause(true)).await.unwrap();
    let original = f.player.queue(&f.token()).await.unwrap();
    let before = f.command(Cmd::Seek(15.0)).await.unwrap();
    let reject = "CREATE TRIGGER reject_intelligence BEFORE INSERT ON state WHEN NEW.kind='playback-intelligence-v1' BEGIN SELECT RAISE(ABORT,'synthetic intelligence write failure'); END;";
    f.sql(reject);
    assert!(act(&f, begin(8, true)).await.is_err());
    assert_eq!(f.player.queue(&f.token()).await.unwrap(), original);
    assert_eq!(
        f.command(Cmd::Snapshot).await.unwrap().play_id,
        before.play_id
    );
    assert!(
        f.store
            .get("9", "playback-intelligence-v1", "session")
            .unwrap()
            .is_none()
    );
    f.sql("DROP TRIGGER reject_intelligence");
    let seeded = act(&f, begin(8, true)).await.unwrap();
    let id = seeded.intelligence.unwrap().id;
    let raw_queue = f.store.get("9", "playback", "queue").unwrap();
    let raw_session = f
        .store
        .get("9", "playback-intelligence-v1", "session")
        .unwrap();
    let loads = f.device.state.lock().unwrap().loads;
    f.sql(reject);
    assert!(act(&f, complete(&id, &[90, 91])).await.is_err());
    assert_eq!(f.store.get("9", "playback", "queue").unwrap(), raw_queue);
    assert_eq!(
        f.store
            .get("9", "playback-intelligence-v1", "session")
            .unwrap(),
        raw_session
    );
    assert_eq!(f.device.state.lock().unwrap().loads, loads);
    assert_eq!(
        f.command(Cmd::Snapshot)
            .await
            .unwrap()
            .intelligence
            .unwrap()
            .phase,
        IntelligencePhase::Seeded
    );
    f.sql("DROP TRIGGER reject_intelligence");
    assert_eq!(
        act(&f, complete(&id, &[90, 91]))
            .await
            .unwrap()
            .intelligence
            .unwrap()
            .phase,
        IntelligencePhase::Active
    );
}

#[tokio::test]
async fn completing_after_stop_installs_recommendations_without_starting_or_reloading_audio() {
    let f = Fixture::new();
    let seeded = act(&f, begin(7, false)).await.unwrap();
    let id = seeded.intelligence.unwrap().id;
    f.command(Cmd::Stop).await.unwrap();
    let loads = f.device.state.lock().unwrap().loads;
    let active = act(&f, complete(&id, &[90, 91])).await.unwrap();
    assert_eq!(active.state, PlaybackStatus::Idle);
    assert!(active.play_id.is_none());
    assert_eq!(f.device.state.lock().unwrap().loads, loads);
    assert_eq!(
        active.intelligence.unwrap().phase,
        IntelligencePhase::Active
    );
    assert_eq!(
        ids(&f.player.queue(&f.token()).await.unwrap()),
        vec![json!(7), json!(90), json!(91)]
    );
}

#[tokio::test]
async fn restoring_only_provenance_during_a_held_seed_load_does_not_cancel_or_replay_that_load() {
    let f = Fixture::new();
    f.command(replace(&[7, 8, 9])).await.unwrap();
    f.api.hold.store(7, Ordering::SeqCst);
    // Invalidate a prefetched source so this seed actually holds the HTTP read.
    f.resolver.invalidate(None).unwrap();
    let view = f.command(Cmd::Snapshot).await.unwrap();
    let loading = f.start(Cmd::Intelligence(IntelligenceUpdate {
        expected: view.fence(),
        action: begin(7, true),
    }));
    tokio::time::timeout(Duration::from_secs(4), f.api.started.notified())
        .await
        .unwrap();
    let seeded = f.command(Cmd::Snapshot).await.unwrap();
    let id = seeded.intelligence.unwrap().id;
    act(&f, complete(&id, &[8, 9])).await.unwrap();
    let left = act(
        &f,
        IntelligenceAction::Leave {
            session_id: id,
            mode: PlaybackMode::Sequential,
            retain_missing_current: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(left.selected.unwrap().source, json!({"kind":"test"}));
    assert!(!loading.is_finished());
    let before = f.device.state.lock().unwrap().loads;
    f.api.hold.store(0, Ordering::SeqCst);
    f.api.release.notify_one();
    let loaded = loading.await.unwrap().unwrap();
    assert_eq!(loaded.state, PlaybackStatus::Playing);
    assert_eq!(loaded.selected.unwrap().source, json!({"kind":"test"}));
    assert!(loaded.intelligence.is_none());
    assert_eq!(f.device.state.lock().unwrap().loads, before + 1);
}

#[tokio::test]
async fn a_new_ordinary_queue_and_removing_every_ai_item_both_cancel_late_recommendations() {
    let f = Fixture::new();
    f.command(Cmd::Mode(PlaybackMode::Random)).await.unwrap();
    let id = act(&f, begin(7, false))
        .await
        .unwrap()
        .intelligence
        .unwrap()
        .id;
    let ordinary = f.command(replace(&[70, 71])).await.unwrap();
    assert!(ordinary.intelligence.is_none());
    assert_eq!(ordinary.mode, PlaybackMode::Random);
    assert!(act(&f, complete(&id, &[90])).await.is_err());
    assert_eq!(song(&f.command(Cmd::Snapshot).await.unwrap()), Some(70));
    let id = act(&f, begin(7, true))
        .await
        .unwrap()
        .intelligence
        .unwrap()
        .id;
    let queue = f.player.queue(&f.token()).await.unwrap();
    f.command(Cmd::Remove(
        queue.items.iter().map(|e| e.entry_id.clone()).collect(),
    ))
    .await
    .unwrap();
    assert!(
        f.command(Cmd::Snapshot)
            .await
            .unwrap()
            .intelligence
            .is_none()
    );
    assert!(
        f.store
            .get("9", "playback-intelligence-v1", "session")
            .unwrap()
            .is_none()
    );
    assert!(act(&f, complete(&id, &[90])).await.is_err());
}

#[tokio::test]
async fn switching_accounts_never_installs_old_recommendations_or_exposes_the_old_backup() {
    let f = Fixture::new();
    let id = act(&f, begin(7, false))
        .await
        .unwrap()
        .intelligence
        .unwrap()
        .id;
    let original = f
        .store
        .get("9", "playback-intelligence-v1", "session")
        .unwrap();
    f.session
        .adopt(f.api.clone(), json!({"userId":10}), &f.token())
        .unwrap();
    let other = f.command(Cmd::Snapshot).await.unwrap();
    assert!(other.intelligence.is_none());
    assert_eq!(other.queue_len, 0);
    assert!(act(&f, complete(&id, &[90])).await.is_err());
    assert!(
        f.store
            .get("10", "playback-intelligence-v1", "session")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        f.store
            .get("9", "playback-intelligence-v1", "session")
            .unwrap(),
        original
    );
}

#[tokio::test]
async fn a_competing_native_host_cannot_use_an_older_seed_fence_after_activation() {
    let f = Fixture::new();
    let seeded = act(&f, begin(7, false)).await.unwrap();
    let id = seeded.intelligence.unwrap().id;
    let other = f.another_player();
    let before = other.command(&f.token(), Cmd::Snapshot).await.unwrap();
    act(&f, complete(&id, &[90, 91])).await.unwrap();
    let result = other
        .command(
            &f.token(),
            Cmd::Intelligence(IntelligenceUpdate {
                expected: before.fence(),
                action: complete(&id, &[99]),
            }),
        )
        .await;
    assert!(matches!(
        root(result.as_ref().err().unwrap()),
        Error::QueueChanged
    ));
    assert_eq!(
        ids(&other.queue(&f.token()).await.unwrap()),
        vec![json!(7), json!(90), json!(91)]
    );
}

#[tokio::test]
async fn explicit_experimental_retention_promotes_a_hidden_current_back_to_a_visible_row() {
    let f = Fixture::new();
    let id = act(&f, begin(90, false))
        .await
        .unwrap()
        .intelligence
        .unwrap()
        .id;
    act(
        &f,
        IntelligenceAction::Fail {
            session_id: id.clone(),
            failure: IntelligenceFailure::RequestFailed,
        },
    )
    .await
    .unwrap();
    let before = f.player.queue(&f.token()).await.unwrap();
    assert!(
        !before
            .items
            .iter()
            .find(|e| Some(&e.entry_id) == before.current.as_ref())
            .unwrap()
            .visible_in_queue()
    );
    act(
        &f,
        IntelligenceAction::Leave {
            session_id: id,
            mode: PlaybackMode::Sequential,
            retain_missing_current: true,
        },
    )
    .await
    .unwrap();
    let after = f.player.queue(&f.token()).await.unwrap();
    assert!(after.intelligence.is_none());
    assert_eq!(after.items.len(), 4);
    assert!(after.items.iter().all(|e| e.visible_in_queue()));
    assert_eq!(after.current, before.current);
}
