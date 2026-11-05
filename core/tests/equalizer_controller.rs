#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::{
    Error,
    effects::equalizer::{
        EqualizerPreferences, EqualizerPreset, EqualizerSettings, EqualizerStatus,
    },
    playback::controller::{PlaybackCommand as Cmd, PlaybackStatus},
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
        preset: None,
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
async fn bad_dsp_block_detaches_only_equalizer_and_keeps_current_song_advancing() {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    f.command(change(0, 5.0)).await.unwrap();
    let loaded = f.command(replace(&[1])).await.unwrap();
    f.device
        .equalizer_failed_block
        .store(true, Ordering::SeqCst);
    let failed = until(&f.player, |s| s.equalizer.status == EqualizerStatus::Failed).await;
    assert_eq!(failed.state, PlaybackStatus::Playing);
    assert_eq!(failed.play_id, loaded.play_id);
    assert_eq!(failed.equalizer.preferences, loaded.equalizer.preferences);
    assert!(f.device.equalizer.lock().unwrap().is_none());

    f.device.state.lock().unwrap().view.position = Some(5.0);
    let advanced = until(&f.player, |s| s.position == Some(5.0)).await;
    assert_eq!(advanced.play_id, loaded.play_id);
    assert_eq!(advanced.equalizer.status, EqualizerStatus::Failed);
    assert_eq!(f.device.state.lock().unwrap().loads, 1);

    f.device
        .equalizer_failed_block
        .store(false, Ordering::SeqCst);
    let retried = f.command(Cmd::RetryEqualizer).await.unwrap();
    assert_eq!(retried.equalizer.status, EqualizerStatus::Configured);
    assert_eq!(retried.play_id, loaded.play_id);
    assert_eq!(*f.device.equalizer.lock().unwrap(), Some(curve(5.0)));
}

#[tokio::test]
async fn bad_dsp_block_stops_transport_when_native_detach_fails() {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    f.command(change(0, 5.0)).await.unwrap();
    f.command(replace(&[1])).await.unwrap();
    f.device.equalizer_clear_fail.store(true, Ordering::SeqCst);
    f.device
        .equalizer_failed_block
        .store(true, Ordering::SeqCst);
    let failed = until(&f.player, |s| s.state == PlaybackStatus::Error).await;
    assert_eq!(failed.equalizer.status, EqualizerStatus::Failed);
    assert!(failed.play_id.is_none());
    assert!(f.device.state.lock().unwrap().view.paused);
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
            preset: None,
        })
        .await
        .unwrap();
    assert!(view.equalizer.preferences.settings.enabled());
    assert!(!f.device.equalizer.lock().unwrap().unwrap().enabled());
}

#[tokio::test]
async fn saved_custom_curve_is_account_owned_revision_guarded_and_separate_from_active_audio() {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    let active = f.command(change(0, 4.0)).await.unwrap();
    f.device.equalizer_fail.store(true, Ordering::SeqCst);
    let saved = f
        .command(Cmd::SaveCustomEqualizer {
            expected_revision: 1,
        })
        .await
        .unwrap();
    assert_eq!(saved.control_revision, active.control_revision);
    assert_eq!(saved.equalizer.preferences.revision, 1);
    assert_eq!(saved.equalizer.preferences.settings, curve(4.0));
    assert_eq!(saved.equalizer.preferences.saved_custom, Some(curve(4.0)));
    assert_eq!(saved.equalizer.status, EqualizerStatus::Configured);
    assert_eq!(*f.device.equalizer.lock().unwrap(), Some(curve(4.0)));

    f.device.equalizer_fail.store(false, Ordering::SeqCst);
    f.command(change(1, -3.0)).await.unwrap();
    let err = f
        .command(Cmd::SaveCustomEqualizer {
            expected_revision: 1,
        })
        .await
        .err()
        .unwrap();
    assert!(matches!(
        root(&err),
        Error::State("equalizer settings changed")
    ));
    let reopened = f
        .another_player()
        .command(&f.token(), Cmd::Snapshot)
        .await
        .unwrap();
    assert_eq!(reopened.equalizer.preferences.settings, curve(-3.0));
    assert_eq!(
        reopened.equalizer.preferences.saved_custom,
        Some(curve(4.0))
    );
    assert_eq!(
        EqualizerPreferences::read(&f.store, "other")
            .unwrap()
            .saved_custom,
        None
    );
    let old = f.token();
    f.session
        .adopt(f.api.clone(), json!({"userId":10}), &old)
        .unwrap();
    let new_owner = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(new_owner.account.uid.as_deref(), Some("10"));
    assert_eq!(new_owner.equalizer.preferences.saved_custom, None);
    assert_eq!(
        EqualizerPreferences::read(&f.store, "9")
            .unwrap()
            .saved_custom,
        Some(curve(4.0))
    );
    assert!(f.api.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn peer_replacing_the_same_active_curve_still_reapplies_its_new_revision() {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    let initial = f.command(change(0, 4.0)).await.unwrap();
    assert_eq!(initial.equalizer.status, EqualizerStatus::Configured);
    let peer = f.another_player();

    // The local controller can observe the peer's revision on its periodic
    // tick before the peer command returns. Arm this device first.
    f.device.equalizer_fail.store(true, Ordering::SeqCst);
    peer.command(&f.token(), change(1, 4.0)).await.unwrap();
    let synced = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(synced.equalizer.preferences.revision, 2);
    assert_eq!(synced.equalizer.preferences.settings, curve(4.0));
    assert_eq!(synced.equalizer.status, EqualizerStatus::Failed);
}

#[tokio::test]
async fn tone_only_curve_is_applied_and_saved_preset_does_not_include_tone() {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    let enhanced = EqualizerSettings::new(true, [0.0; 10])
        .unwrap()
        .with_tone(6, -3)
        .unwrap();
    let first = f
        .command(Cmd::Equalizer {
            expected_revision: 0,
            settings: enhanced,
            preset: None,
        })
        .await
        .unwrap();
    assert_eq!(first.equalizer.preferences.settings.tone(), (6, -3));
    assert_eq!(*f.device.equalizer.lock().unwrap(), Some(enhanced));
    let saved = f
        .command(Cmd::SaveCustomEqualizer {
            expected_revision: 1,
        })
        .await
        .unwrap();
    assert_eq!(
        saved.equalizer.preferences.saved_custom.unwrap().tone(),
        (0, 0)
    );
    assert_eq!(
        EqualizerPreferences::read(&f.store, "9").unwrap().settings,
        enhanced
    );
    let old: EqualizerSettings =
        serde_json::from_value(json!({"enabled":true,"gains":[0,0,0,0,0,0,0,0,0,0]})).unwrap();
    assert_eq!(old.tone(), (0, 0));
    assert!(
        serde_json::from_value::<EqualizerSettings>(
            json!({"enabled":true,"gains":[0,0,0,0,0,0,0,0,0,0],"bass_db":11})
        )
        .is_err()
    );
}

#[tokio::test]
async fn manual_flat_can_be_saved_and_identical_builtin_selected_without_touching_dsp() {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    let flat = EqualizerSettings::new(true, [0.0; 10]).unwrap();
    let select = |expected_revision, preset| Cmd::Equalizer {
        expected_revision,
        settings: flat,
        preset: Some(preset),
    };
    let none = EqualizerPreset::BuiltIn { index: 0 };
    let first = f.command(select(0, none)).await.unwrap();
    assert_eq!(first.equalizer.status, EqualizerStatus::Configured);

    // A fresh apply would fail and clear the filter, even with equal gains.
    f.device.equalizer_fail.store(true, Ordering::SeqCst);
    let manual = f.command(select(1, EqualizerPreset::Manual)).await.unwrap();
    assert_eq!(
        manual.equalizer.preferences.preset,
        Some(EqualizerPreset::Manual)
    );
    assert_eq!(manual.equalizer.status, EqualizerStatus::Configured);
    assert!(
        f.command(Cmd::SaveCustomEqualizer {
            expected_revision: 1
        })
        .await
        .is_err()
    );
    let saved = f
        .command(Cmd::SaveCustomEqualizer {
            expected_revision: 2,
        })
        .await
        .unwrap();
    assert_eq!(
        saved.equalizer.preferences.preset,
        Some(EqualizerPreset::Saved)
    );
    assert_eq!(saved.equalizer.preferences.revision, 3);
    assert_eq!(saved.equalizer.preferences.saved_custom, Some(flat));
    let builtin = f
        .command(Cmd::Equalizer {
            expected_revision: 3,
            settings: flat.with_tone_enabled(true),
            preset: Some(none),
        })
        .await
        .unwrap();
    assert_eq!(builtin.equalizer.status, EqualizerStatus::Configured);
    assert_eq!(builtin.equalizer.preferences.preset, Some(none));
    assert_eq!(builtin.equalizer.preferences.saved_custom, Some(flat));
    let reopened = f.another_player();
    let recovered = reopened.command(&f.token(), Cmd::Snapshot).await.unwrap();
    assert_eq!(
        recovered.equalizer.preferences,
        builtin.equalizer.preferences
    );
    assert_eq!(
        EqualizerPreferences::read(&f.store, "other").unwrap(),
        EqualizerPreferences::default()
    );
}

#[tokio::test]
async fn migrated_bypass_and_independent_tone_reach_only_the_requested_dsp_stage() {
    let fixture = Fixture::new();
    fixture
        .device
        .equalizer_supported
        .store(true, Ordering::SeqCst);
    let gains = *curve(4.0).gains();
    let legacy: EqualizerSettings =
        serde_json::from_value(json!({"enabled":false,"gains":gains,"bass_db":6})).unwrap();
    let write = |revision, settings| Cmd::Equalizer {
        expected_revision: revision,
        settings,
        preset: Some(EqualizerPreset::Manual),
    };
    assert!(!legacy.tone_enabled());
    fixture.command(write(0, legacy)).await.unwrap();
    assert_eq!(
        *fixture.device.equalizer.lock().unwrap(),
        Some(EqualizerSettings::default())
    );

    let independent = legacy.with_tone_enabled(true);
    let view = fixture.command(write(1, independent)).await.unwrap();
    assert_eq!(view.equalizer.status, EqualizerStatus::Configured);
    assert_eq!(view.equalizer.preferences.settings, independent);
    assert_eq!(view.equalizer.preferences.settings.gains(), &gains);
    let tone_only = EqualizerSettings::new(true, [0.0; 10])
        .unwrap()
        .with_tone(6, 0)
        .unwrap();
    assert_eq!(*fixture.device.equalizer.lock().unwrap(), Some(tone_only));

    let device = std::sync::Arc::new(Device::default());
    device.equalizer_supported.store(true, Ordering::SeqCst);
    let reopened = cumulus_core::playback::controller::PlaybackController::start(
        fixture.session.clone(),
        fixture.store.clone(),
        fixture.resolver.clone(),
        audio_engine(device.clone()),
    )
    .unwrap();
    let restored = reopened
        .command(&fixture.token(), Cmd::Snapshot)
        .await
        .unwrap();
    assert_eq!(restored.equalizer.preferences.settings, independent);
    assert_eq!(*device.equalizer.lock().unwrap(), Some(tone_only));

    // Explicit flat GEQ reset retains the enhancement, while resetting the
    // enhancement with a nonflat GEQ curve leaves that curve processing.
    let flat = tone_only.with_tone_enabled(true);
    reopened
        .command(&fixture.token(), write(2, flat))
        .await
        .unwrap();
    assert_eq!(*device.equalizer.lock().unwrap(), Some(tone_only));
    let restored_geq = curve(4.0).with_tone_enabled(false);
    reopened
        .command(&fixture.token(), write(3, restored_geq))
        .await
        .unwrap();
    assert_eq!(*device.equalizer.lock().unwrap(), Some(curve(4.0)));
}

#[tokio::test]
async fn master_bypass_preserves_the_mixed_preset_and_loudness_across_restart() {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    f.device.loudness_supported.store(true, Ordering::SeqCst);
    let loudness = f
        .command(Cmd::Loudness {
            expected_revision: "initial".into(),
            enabled: true,
        })
        .await
        .unwrap()
        .loudness;
    f.api.gains.lock().unwrap().insert(1, json!(-12));
    f.command(replace(&[1])).await.unwrap();
    let live_loudness = *f.device.loudness.lock().unwrap();
    assert_eq!(live_loudness.unwrap().gain_db(), -12.0);
    let preset = Some(EqualizerPreset::BuiltIn { index: 1 });
    let mixed = curve(4.0).with_tone(6, -3).unwrap();
    let write = |expected_revision, settings| Cmd::Equalizer {
        expected_revision,
        settings,
        preset,
    };
    let active = f.command(write(0, mixed)).await.unwrap();
    let mut wire = serde_json::to_value(mixed).unwrap();
    wire["master_bypassed"] = json!(true);
    let bypassed = serde_json::from_value(wire.clone()).unwrap();
    let off = f.command(write(1, bypassed)).await.unwrap();
    assert_eq!(off.control_revision, active.control_revision);
    assert_eq!(off.equalizer.preferences.settings, bypassed);
    assert_eq!(off.equalizer.preferences.preset, preset);
    assert_eq!(off.loudness.preferences, loudness.preferences);
    assert_eq!(*f.device.loudness.lock().unwrap(), live_loudness);
    assert_eq!(
        *f.device.equalizer.lock().unwrap(),
        Some(EqualizerSettings::default())
    );

    let device = std::sync::Arc::new(Device::default());
    device.equalizer_supported.store(true, Ordering::SeqCst);
    let reopened = cumulus_core::playback::controller::PlaybackController::start(
        f.session.clone(),
        f.store.clone(),
        f.resolver.clone(),
        audio_engine(device.clone()),
    )
    .unwrap();
    let snapshot = reopened.command(&f.token(), Cmd::Snapshot).await.unwrap();
    assert_eq!(snapshot.equalizer.preferences, off.equalizer.preferences);
    assert_eq!(snapshot.loudness.preferences, loudness.preferences);
    assert_eq!(
        *device.equalizer.lock().unwrap(),
        Some(EqualizerSettings::default())
    );
    let resumed = reopened.command(&f.token(), write(2, mixed)).await.unwrap();
    assert_eq!(resumed.equalizer.preferences.preset, preset);
    assert_eq!(resumed.loudness.preferences, loudness.preferences);
    assert_eq!(*device.equalizer.lock().unwrap(), Some(mixed));
}

#[tokio::test]
async fn an_unconfirmed_disable_persists_the_dry_request_and_can_be_retried() {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    let enabled = f.command(change(0, 4.0)).await.unwrap();
    assert_eq!(enabled.equalizer.status, EqualizerStatus::Configured);
    let disabled = EqualizerSettings::new(false, *curve(4.0).gains()).unwrap();
    f.device.equalizer_fail.store(true, Ordering::SeqCst);
    let failed = f
        .command(Cmd::Equalizer {
            expected_revision: enabled.equalizer.preferences.revision,
            settings: disabled,
            preset: None,
        })
        .await
        .unwrap();
    assert_eq!(failed.equalizer.status, EqualizerStatus::Failed);
    assert_eq!(failed.equalizer.preferences.settings, disabled);
    assert_eq!(
        EqualizerPreferences::read(&f.store, "9").unwrap().settings,
        disabled
    );
    assert!(f.device.equalizer.lock().unwrap().is_none());
    f.device.equalizer_fail.store(false, Ordering::SeqCst);
    let retried = f.command(Cmd::RetryEqualizer).await.unwrap();
    assert_eq!(retried.equalizer.status, EqualizerStatus::Configured);
    assert_eq!(retried.equalizer.preferences, failed.equalizer.preferences);
    assert!(!f.device.equalizer.lock().unwrap().unwrap().enabled());
}

#[path = "support/equalizer_source_latch.rs"]
mod source_latch;
