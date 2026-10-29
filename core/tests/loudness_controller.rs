#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::{
    effects::loudness::{LoudnessPreferences, LoudnessSettings, LoudnessStatus as Status},
    membership::Quality,
    playback::controller::{PlaybackCommand as Cmd, PlaybackStatus},
};
use serde_json::json;
use std::{sync::atomic::Ordering, time::Duration};
use support::*;

fn change(revision: &str, enabled: bool) -> Cmd {
    Cmd::Loudness {
        expected_revision: revision.into(),
        enabled,
    }
}
fn settings(enabled: bool, gain: f32) -> Option<LoudnessSettings> {
    Some(LoudnessSettings::new(enabled, gain).unwrap())
}
#[tokio::test]
async fn each_resolved_source_replaces_gain_before_unpause_and_never_uses_queue_metadata() {
    let f = Fixture::new();
    f.device.loudness_supported.store(true, Ordering::SeqCst);
    f.api.gains.lock().unwrap().extend([
        (1, json!(-12.34567)),
        (2, json!(0)),
        (3, json!("-18")),
        (4, json!(10_000)),
    ]);
    let before = f.command(Cmd::Snapshot).await.unwrap();
    let enabled = f.command(change("initial", true)).await.unwrap();
    assert_eq!(enabled.control_revision, before.control_revision);
    assert_eq!(enabled.loudness.status, Status::NoTrack);
    assert!(f.api.calls.lock().unwrap().is_empty());
    let loaded = f.command(replace(&[1, 2, 3, 4])).await.unwrap();
    assert_eq!(loaded.loudness.status, Status::Configured);
    assert_eq!(
        f.device.loudness_at_unpause.lock().unwrap().last(),
        Some(&settings(true, -12.3456))
    );
    for expected in [Status::NoGain, Status::NoGain, Status::InvalidGain] {
        let next = f.command(Cmd::Next).await.unwrap();
        assert_eq!(next.loudness.status, expected);
        assert_eq!(
            f.device.loudness_at_unpause.lock().unwrap().last(),
            Some(&settings(false, 0.0))
        );
    }
    f.command(Cmd::Stop).await.unwrap();
    assert!(f.device.loudness.lock().unwrap().is_none());
}
#[tokio::test]
async fn quality_refresh_uses_new_authorization_and_off_switch_preserves_transport() {
    let f = Fixture::new();
    f.device.loudness_supported.store(true, Ordering::SeqCst);
    f.api.gains.lock().unwrap().insert(1, json!(-18));
    let enabled = f.command(change("initial", true)).await.unwrap();
    f.command(replace(&[1])).await.unwrap();
    f.api.gains.lock().unwrap().insert(1, json!(-9));
    let changed = f.command(Cmd::Quality(Quality::Lossless)).await.unwrap();
    assert_eq!(changed.loudness.gain.unwrap().decibels(), -9.0);
    assert_eq!(
        f.device.loudness_at_unpause.lock().unwrap().last(),
        Some(&settings(true, -9.0))
    );
    let disabled = f
        .command(change(&enabled.loudness.preferences.revision, false))
        .await
        .unwrap();
    assert_eq!(disabled.play_id, changed.play_id);
    assert_eq!(disabled.control_revision, changed.control_revision);
    assert_eq!(disabled.state, PlaybackStatus::Playing);
    assert_eq!(disabled.loudness.status, Status::Off);
    assert_eq!(*f.device.loudness.lock().unwrap(), settings(false, -9.0));
    f.command(Cmd::Stop).await.unwrap();
}
#[tokio::test]
async fn save_during_blocked_decode_is_nonblocking_and_latest_setting_precedes_unpause() {
    let f = Fixture::new();
    f.device.loudness_supported.store(true, Ordering::SeqCst);
    f.api.gains.lock().unwrap().insert(1, json!(-18));
    f.device.hold_load.store(true, Ordering::SeqCst);
    let loading = f.start(replace(&[1]));
    f.device.started.notified().await;
    let saved = tokio::time::timeout(Duration::from_secs(1), f.command(change("initial", true)))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.loudness.status, Status::Pending);
    f.device.hold_load.store(false, Ordering::SeqCst);
    let loaded = loading.await.unwrap().unwrap();
    assert_eq!(loaded.loudness.preferences, saved.loudness.preferences);
    assert_eq!(
        f.device.loudness_at_unpause.lock().unwrap().last(),
        Some(&settings(true, -18.0))
    );
    f.command(Cmd::Stop).await.unwrap();
}
#[tokio::test]
async fn device_preference_survives_account_switch_but_old_gain_and_late_work_do_not() {
    let f = Fixture::new();
    f.device.loudness_supported.store(true, Ordering::SeqCst);
    f.api.gains.lock().unwrap().insert(1, json!(-18));
    let saved = f.command(change("initial", true)).await.unwrap();
    f.command(replace(&[1])).await.unwrap();
    let old = f.token();
    f.session
        .adopt(f.api.clone(), json!({"userId":10}), &old)
        .unwrap();
    let new = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(new.loudness.preferences, saved.loudness.preferences);
    assert_eq!(new.loudness.status, Status::NoTrack);
    assert!(new.loudness.gain.is_none() && f.device.loudness.lock().unwrap().is_none());
    assert!(
        f.player
            .command(&old, change(&saved.loudness.preferences.revision, false))
            .await
            .is_err()
    );
    let reopened = f.another_player();
    let value = reopened.command(&f.token(), Cmd::Snapshot).await.unwrap();
    assert_eq!(value.loudness.preferences, saved.loudness.preferences);
    assert_eq!(value.loudness.status, Status::NoTrack);
}
#[tokio::test]
async fn failed_or_unavailable_filter_is_explicit_and_retry_does_not_duplicate_playback() {
    let f = Fixture::new();
    f.api.gains.lock().unwrap().insert(1, json!(-18));
    f.command(change("initial", true)).await.unwrap();
    let unavailable = f.command(replace(&[1])).await.unwrap();
    assert_eq!(unavailable.loudness.status, Status::Unavailable);
    f.device.loudness_supported.store(true, Ordering::SeqCst);
    f.device.loudness_fail.store(true, Ordering::SeqCst);
    let failed = f.command(Cmd::RetryLoudness).await.unwrap();
    assert_eq!(failed.loudness.status, Status::Failed);
    assert!(f.device.loudness.lock().unwrap().is_none());
    assert_eq!(failed.state, PlaybackStatus::Playing);
    f.device.loudness_fail.store(false, Ordering::SeqCst);
    let retried = f.command(Cmd::RetryLoudness).await.unwrap();
    assert_eq!(retried.loudness.status, Status::Configured);
    assert_eq!(retried.play_id, unavailable.play_id);
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    f.device.loudness_fail.store(true, Ordering::SeqCst);
    f.device.loudness_clear_fail.store(true, Ordering::SeqCst);
    assert!(f.command(Cmd::RetryLoudness).await.is_err());
    assert!(f.device.state.lock().unwrap().view.paused);
    f.device.loudness_clear_fail.store(false, Ordering::SeqCst);
    f.command(Cmd::Stop).await.unwrap();
}
#[tokio::test]
async fn damaged_preference_disables_processing_and_repair_never_revives_stale_cas() {
    let f = Fixture::new();
    f.device.loudness_supported.store(true, Ordering::SeqCst);
    f.api.gains.lock().unwrap().insert(1, json!(-18));
    let saved = f.command(change("initial", true)).await.unwrap();
    f.command(replace(&[1])).await.unwrap();
    f.store
        .put(
            "local-device",
            "loudness-preferences-v1",
            "current",
            &json!({"enabled":"bad"}),
        )
        .unwrap();
    let damaged = f.command(Cmd::Snapshot).await.unwrap();
    assert!(damaged.loudness.damaged);
    assert_eq!(damaged.loudness.status, Status::Failed);
    assert_eq!(*f.device.loudness.lock().unwrap(), settings(false, -18.0));
    let repaired = f.command(Cmd::RepairLoudness).await.unwrap();
    assert_eq!(repaired.loudness.status, Status::Off);
    assert!(!repaired.loudness.damaged);
    assert!(
        f.command(change(&saved.loudness.preferences.revision, true))
            .await
            .is_err()
    );
    assert!(!LoudnessPreferences::read(&f.store).unwrap().enabled);
    f.command(Cmd::Stop).await.unwrap();
}
