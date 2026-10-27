#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::{
    Error,
    effects::equalizer::{EqualizerPreferences, EqualizerSettings, EqualizerStatus},
    playback::controller::PlaybackCommand as Cmd,
};
use serde_json::json;
use std::{sync::atomic::Ordering, time::Duration};
use support::*;
fn curve(gain: f32) -> EqualizerSettings {
    let mut values = [0.0; 10];
    values[5] = gain;
    EqualizerSettings::new(true, values).unwrap()
}
fn change(revision: u64, gain: f32) -> Cmd {
    Cmd::Equalizer {
        expected_revision: revision,
        settings: curve(gain),
    }
}

#[tokio::test]
async fn account_owned_desired_curve_survives_restart_without_changing_transport_fence() {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    let before = f.command(Cmd::Snapshot).await.unwrap();
    let after = f.command(change(0, 4.0)).await.unwrap();
    assert_eq!(after.control_revision, before.control_revision);
    assert_eq!(after.equalizer.status, EqualizerStatus::Configured);
    assert_eq!(*f.device.equalizer.lock().unwrap(), Some(curve(4.0)));
    assert_eq!(
        EqualizerPreferences::read(&f.store, "9").unwrap().settings,
        curve(4.0)
    );
    assert!(f.api.calls.lock().unwrap().is_empty());
    let other = f.another_player();
    let view = other.command(&f.token(), Cmd::Snapshot).await.unwrap();
    assert_eq!(view.equalizer.preferences, after.equalizer.preferences);
    assert_eq!(view.equalizer.status, EqualizerStatus::Unavailable);
    let device = std::sync::Arc::new(Device::default());
    device.equalizer_supported.store(true, Ordering::SeqCst);
    let restored = cumulus_core::playback::controller::PlaybackController::start(
        f.session.clone(),
        f.store.clone(),
        f.resolver.clone(),
        audio_engine(device.clone()),
    )
    .unwrap();
    let view = restored.command(&f.token(), Cmd::Snapshot).await.unwrap();
    assert_eq!(view.equalizer.status, EqualizerStatus::Configured);
    assert_eq!(*device.equalizer.lock().unwrap(), Some(curve(4.0)));
}
#[tokio::test]
async fn unavailable_and_failed_application_are_saved_and_explicitly_retried() {
    let f = Fixture::new();
    let saved = f.command(change(0, 5.0)).await.unwrap();
    assert_eq!(saved.equalizer.status, EqualizerStatus::Unavailable);
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    f.device.equalizer_fail.store(true, Ordering::SeqCst);
    let failed = f.command(Cmd::RetryEqualizer).await.unwrap();
    assert_eq!(failed.equalizer.status, EqualizerStatus::Failed);
    assert!(f.device.equalizer.lock().unwrap().is_none());
    assert_eq!(failed.equalizer.preferences, saved.equalizer.preferences);
    f.device.equalizer_fail.store(false, Ordering::SeqCst);
    let retried = f.command(Cmd::RetryEqualizer).await.unwrap();
    assert_eq!(retried.equalizer.status, EqualizerStatus::Configured);
    assert_eq!(retried.equalizer.preferences.revision, 1);
    assert_eq!(*f.device.equalizer.lock().unwrap(), Some(curve(5.0)));
}
#[tokio::test]
async fn stale_revision_cannot_overwrite_a_second_controller_or_the_native_curve() {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    f.command(change(0, 2.0)).await.unwrap();
    let other = f.another_player();
    other.command(&f.token(), change(1, -3.0)).await.unwrap();
    assert!(matches!(
        root(&f.command(change(1, 10.0)).await.err().unwrap()),
        Error::State("equalizer settings changed")
    ));
    assert_eq!(
        EqualizerPreferences::read(&f.store, "9").unwrap().settings,
        curve(-3.0)
    );
    assert_eq!(*f.device.equalizer.lock().unwrap(), Some(curve(-3.0)));
}
#[tokio::test]
async fn a_curve_saved_during_decode_applies_before_first_unpause_without_blocking_cancel() {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    f.device.hold_load.store(true, Ordering::SeqCst);
    let loading = f.start(replace(&[1]));
    f.device.started.notified().await;
    let changed = tokio::time::timeout(Duration::from_secs(1), f.command(change(0, 6.0)))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(changed.equalizer.status, EqualizerStatus::Pending);
    f.device.hold_load.store(false, Ordering::SeqCst);
    loading.await.unwrap().unwrap();
    assert_eq!(
        f.device.equalizer_at_unpause.lock().unwrap().last(),
        Some(&Some(curve(6.0)))
    );
    f.command(Cmd::Stop).await.unwrap();
}
#[tokio::test]
async fn switching_accounts_clears_old_curve_and_rejects_late_configuration() {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    f.command(Cmd::Snapshot).await.unwrap();
    // Drain the initial configuration notification.
    let _ = tokio::time::timeout(
        Duration::from_millis(10),
        f.device.equalizer_started.notified(),
    )
    .await;
    f.device.equalizer_hold.store(true, Ordering::SeqCst);
    let changing = f.start(change(0, 7.0));
    f.device.equalizer_started.notified().await;
    let old = f.token();
    f.session
        .adopt(f.api.clone(), json!({"userId":10}), &old)
        .unwrap();
    f.device.equalizer_hold.store(false, Ordering::SeqCst);
    assert!(changing.await.unwrap().is_err());
    let current = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(current.account.uid.as_deref(), Some("10"));
    assert_eq!(
        current.equalizer.preferences.settings,
        EqualizerSettings::default()
    );
    assert_eq!(
        *f.device.equalizer.lock().unwrap(),
        Some(EqualizerSettings::default())
    );
    assert_eq!(
        EqualizerPreferences::read(&f.store, "9").unwrap().settings,
        curve(7.0)
    );
    assert_eq!(
        EqualizerPreferences::read(&f.store, "10").unwrap().revision,
        0
    );
    assert!(f.device.equalizer_clears.load(Ordering::SeqCst) > 0);
}
#[tokio::test]
async fn corrupt_new_account_preferences_clear_old_audio_and_are_not_overwritten() {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    f.command(change(0, 8.0)).await.unwrap();
    let bad = json!({"revision":1,"settings":{"enabled":true,"gains":[99,0,0,0,0,0,0,0,0,0]}});
    f.store
        .put("10", "equalizer-preferences-v1", "current", &bad)
        .unwrap();
    f.session
        .adopt(f.api.clone(), json!({"userId":10}), &f.token())
        .unwrap();
    let view = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(view.equalizer.status, EqualizerStatus::Failed);
    assert!(view.equalizer.damaged);
    assert_eq!(
        *f.device.equalizer.lock().unwrap(),
        Some(EqualizerSettings::default())
    );
    assert_eq!(
        f.store
            .get("10", "equalizer-preferences-v1", "current")
            .unwrap(),
        Some(bad.clone())
    );
    assert_eq!(
        f.player.snapshot().equalizer.preferences.settings,
        EqualizerSettings::default()
    );
    let repaired = f.command(Cmd::RepairEqualizer).await.unwrap();
    assert!(!repaired.equalizer.damaged);
    assert_eq!(repaired.equalizer.status, EqualizerStatus::Configured);
    assert_eq!(
        f.store
            .values("10", "equalizer-recovery-v1")
            .unwrap()
            .into_values()
            .collect::<Vec<_>>(),
        vec![bad]
    );
    assert!(f.command(Cmd::RepairEqualizer).await.is_err());
}

#[tokio::test]
async fn storage_failure_cannot_change_the_native_curve_or_durable_revision() {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    let before = f.command(change(0, 3.0)).await.unwrap();
    f.sql("CREATE TRIGGER deny_equalizer BEFORE UPDATE ON state WHEN NEW.kind='equalizer-preferences-v1' BEGIN SELECT RAISE(FAIL,'fixture'); END;");
    assert!(f.command(change(1, -5.0)).await.is_err());
    assert_eq!(*f.device.equalizer.lock().unwrap(), Some(curve(3.0)));
    assert_eq!(
        EqualizerPreferences::read(&f.store, "9").unwrap(),
        before.equalizer.preferences
    );
}
#[tokio::test]
async fn flat_curve_keeps_overall_switch_but_bypasses_the_recovered_dsp() {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    let flat = EqualizerSettings::new(true, [0.0; 10]).unwrap();
    let view = f
        .command(Cmd::Equalizer {
            expected_revision: 0,
            settings: flat,
        })
        .await
        .unwrap();
    assert!(view.equalizer.preferences.settings.enabled());
    assert!(!f.device.equalizer.lock().unwrap().unwrap().enabled());
}
