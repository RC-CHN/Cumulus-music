#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::{
    Error,
    playback::{
        PlaybackQueue,
        controller::{PlaybackCommand as Cmd, PlaybackStatus as Status, StartupRestore},
        preferences::{StartupPreferences, StartupSettings},
    },
};
use serde_json::json;
use std::sync::atomic::Ordering;
use support::*;

fn seed(f: &Fixture, owner: &str, position: f64) {
    let mut queue = PlaybackQueue::new(Some(f.store.clone()), owner).unwrap();
    queue
        .replace(vec![json!({"id":1,"dt":120000})], 0, json!({"kind":"test"}))
        .unwrap();
    f.store.put(owner, "playback", "position", &json!({"entry_id":queue.selected().unwrap().entry_id,"seconds":position,"state":"paused","timeline_version":1})).unwrap();
}
async fn input(f: &Fixture, auto_play: bool, remember_position: bool) -> StartupRestore {
    StartupRestore {
        expected: f.command(Cmd::Snapshot).await.unwrap().fence(),
        queue: None,
        settings: StartupSettings {
            auto_play,
            remember_position,
        },
        allow_autoplay: true,
    }
}

#[tokio::test]
async fn startup_applies_all_four_official_preferences_and_defaults() {
    assert_eq!(
        StartupSettings::default(),
        StartupSettings {
            auto_play: false,
            remember_position: true
        }
    );
    for auto in [false, true] {
        for remember in [false, true] {
            let f = Fixture::new();
            seed(&f, "9", 42.5);
            let result = f
                .command(Cmd::Startup(input(&f, auto, remember).await))
                .await
                .unwrap();
            assert_eq!(
                result.state,
                if auto {
                    Status::Playing
                } else {
                    Status::Paused
                }
            );
            assert_eq!(result.position, Some(if remember { 42.5 } else { 0.0 }));
            assert!(!result.startup_pending && !result.startup_autoplay);
            assert_eq!(f.device.state.lock().unwrap().loads, 1);
            f.command(Cmd::Stop).await.unwrap();
        }
    }
}

#[tokio::test]
async fn startup_never_replays_after_reconnect_stop_or_explicit_transport() {
    let f = Fixture::new();
    seed(&f, "9", 12.0);
    let initial = f
        .command(Cmd::Startup(input(&f, true, true).await))
        .await
        .unwrap();
    let duplicate = f
        .command(Cmd::Startup(input(&f, true, true).await))
        .await
        .unwrap();
    assert_eq!(duplicate.play_id, initial.play_id);
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    f.command(Cmd::Stop).await.unwrap();
    assert_eq!(
        f.command(Cmd::Startup(input(&f, true, true).await))
            .await
            .unwrap()
            .state,
        Status::Idle
    );
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    assert_eq!(f.command(Cmd::Restore).await.unwrap().state, Status::Paused);
    let other = Fixture::new();
    seed(&other, "9", 44.0);
    other.command(Cmd::Stop).await.unwrap();
    assert_eq!(
        other
            .command(Cmd::Startup(input(&other, true, true).await))
            .await
            .unwrap()
            .state,
        Status::Idle
    );
    assert_eq!(other.device.state.lock().unwrap().loads, 0);
}

#[tokio::test]
async fn account_switch_restores_only_its_clock_and_never_autoplays() {
    let f = Fixture::new();
    seed(&f, "9", 42.5);
    seed(&f, "10", 15.0);
    f.command(Cmd::Startup(input(&f, true, true).await))
        .await
        .unwrap();
    let old = f.token();
    f.session
        .adopt(f.api.clone(), json!({"userId":10}), &old)
        .unwrap();
    let switched = f
        .command(Cmd::Startup(input(&f, true, true).await))
        .await
        .unwrap();
    assert_eq!(switched.state, Status::Paused);
    assert_eq!(switched.position, Some(15.0));
    assert_eq!(switched.account.uid.as_deref(), Some("10"));
    assert!(matches!(
        f.player.command(&old, Cmd::Restore).await,
        Err(Error::StaleResponse)
    ));
    f.session
        .adopt(f.api.clone(), json!({"userId":9}), &f.token())
        .unwrap();
    let back = f
        .command(Cmd::Startup(input(&f, true, true).await))
        .await
        .unwrap();
    assert_eq!(back.state, Status::Paused);
    assert_eq!(back.position, Some(42.5));
}

#[tokio::test]
async fn remembered_end_resets_and_disabled_memory_respects_authorized_trial_start() {
    let f = Fixture::new();
    seed(&f, "9", 120.0);
    let result = f
        .command(Cmd::Startup(input(&f, false, true).await))
        .await
        .unwrap();
    assert_eq!(result.position, Some(0.0));
    assert!(result.position_reset);
    let trial = Fixture::new();
    seed(&trial, "9", 42.0);
    trial.api.trial.store(true, Ordering::SeqCst);
    let result = trial
        .command(Cmd::Startup(input(&trial, false, false).await))
        .await
        .unwrap();
    assert_eq!(result.position, Some(30.0));
    assert_eq!(result.timeline.unwrap().start, 30.0);
    let limited = Fixture::new();
    seed(&limited, "9", 42.0);
    let mut request = input(&limited, true, true).await;
    request.allow_autoplay = false;
    assert_eq!(
        limited.command(Cmd::Startup(request)).await.unwrap().state,
        Status::Paused
    );
}

#[tokio::test]
async fn cancelled_startup_and_stale_queue_cannot_restart_the_audio() {
    let f = Fixture::new();
    seed(&f, "9", 17.0);
    f.api.hold.store(1, Ordering::SeqCst);
    let work = f.start(Cmd::Startup(input(&f, true, true).await));
    f.api.started.notified().await;
    f.command(Cmd::Stop).await.unwrap();
    assert!(work.await.unwrap().is_err());
    f.api.release.notify_one();
    assert_eq!(
        f.command(Cmd::Startup(input(&f, true, true).await))
            .await
            .unwrap()
            .state,
        Status::Idle
    );
    assert_eq!(f.device.state.lock().unwrap().loads, 0);
    let other = Fixture::new();
    seed(&other, "9", 12.0);
    let stale = input(&other, true, true).await;
    seed(&other, "9", 23.0);
    assert!(matches!(
        other.command(Cmd::Startup(stale)).await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(other.device.state.lock().unwrap().loads, 0);
}

#[test]
fn startup_preferences_persist_use_cas_and_repair_only_damaged_data() {
    let dir = tempfile::tempdir().unwrap();
    let store = cumulus_core::storage::StateStore::open(dir.path()).unwrap();
    let original = StartupPreferences::read(&store).unwrap();
    assert_eq!(original.revision, 0);
    let next = StartupPreferences::replace(
        &store,
        0,
        StartupSettings {
            auto_play: true,
            remember_position: false,
        },
    )
    .unwrap();
    assert!(StartupPreferences::replace(&store, 0, StartupSettings::default()).is_err());
    assert!(StartupPreferences::repair(&store).is_err());
    let reopened = cumulus_core::storage::StateStore::open(dir.path()).unwrap();
    assert_eq!(StartupPreferences::read(&reopened).unwrap(), next);
    let sql = rusqlite::Connection::open(dir.path().join("core.sqlite3")).unwrap();
    sql.execute_batch("CREATE TRIGGER fail_settings BEFORE INSERT ON state WHEN NEW.kind='playback-startup-preferences-v1' AND NEW.key='current' BEGIN SELECT RAISE(ABORT,'synthetic disk error'); END;").unwrap();
    assert!(StartupPreferences::replace(&store, 1, StartupSettings::default()).is_err());
    assert_eq!(StartupPreferences::read(&store).unwrap(), next);
    assert_eq!(
        store
            .get(
                "local-device",
                "playback-startup-preferences-v1",
                "revision"
            )
            .unwrap(),
        Some(json!(1))
    );
    sql.execute_batch("DROP TRIGGER fail_settings").unwrap();
    store
        .put(
            "local-device",
            "playback-startup-preferences-v1",
            "current",
            &json!({"revision":7,"settings":{"autoPlay":"invalid"}}),
        )
        .unwrap();
    assert!(StartupPreferences::read(&store).is_err());
    let repaired = StartupPreferences::repair(&store).unwrap();
    assert_eq!(repaired.revision, 8);
    assert_eq!(repaired.settings, StartupSettings::default());
    assert!(StartupPreferences::replace(&store, 1, next.settings).is_err());
    assert_eq!(StartupPreferences::read(&store).unwrap(), repaired);
    store
        .put(
            "local-device",
            "playback-startup-preferences-v1",
            "current",
            &json!({"broken":true}),
        )
        .unwrap();
    let repaired_again = StartupPreferences::repair(&store).unwrap();
    assert_eq!(repaired_again.revision, 9);
    assert!(StartupPreferences::replace(&store, 8, next.settings).is_err());
    assert!(StartupPreferences::repair(&store).is_err());
    let archived: i64 = sql.query_row("SELECT count(*) FROM state WHERE owner='local-device' AND kind='playback-startup-recovery-v1'", [], |row|row.get(0)).unwrap();
    assert_eq!(archived, 2);
    store
        .put(
            "local-device",
            "playback-startup-preferences-v1",
            "revision",
            &json!("broken"),
        )
        .unwrap();
    assert!(StartupPreferences::replace(&store, 9, next.settings).is_err());
    assert_eq!(StartupPreferences::read(&store).unwrap(), repaired_again);
    store
        .put(
            "local-device",
            "playback-startup-preferences-v1",
            "current",
            &json!({"broken":true}),
        )
        .unwrap();
    assert!(StartupPreferences::repair(&store).is_err());
    let archived: i64 = sql.query_row("SELECT count(*) FROM state WHERE owner='local-device' AND kind='playback-startup-recovery-v1'", [], |row|row.get(0)).unwrap();
    assert_eq!(archived, 2);
}
