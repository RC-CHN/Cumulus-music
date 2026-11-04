#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;

use cumulus_core::{
    effects::{
        equalizer::{EqualizerSettings, EqualizerStatus},
        loudness::LoudnessStatus,
    },
    playback::controller::{PlaybackCommand as Cmd, PlaybackSnapshot, PlaybackStatus},
};
use serde_json::json;
use std::sync::atomic::Ordering;
use support::*;

async fn loaded() -> (Fixture, PlaybackSnapshot) {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    f.device.loudness_supported.store(true, Ordering::SeqCst);
    f.api.gains.lock().unwrap().insert(1, json!(-12));
    f.command(Cmd::Equalizer {
        expected_revision: 0,
        settings: EqualizerSettings::new(true, [2.0; 10]).unwrap(),
        preset: None,
    })
    .await
    .unwrap();
    f.command(Cmd::Loudness {
        expected_revision: "initial".into(),
        enabled: true,
    })
    .await
    .unwrap();
    let view = f.command(replace(&[1])).await.unwrap();
    assert_eq!(view.state, PlaybackStatus::Playing);
    assert_eq!(view.equalizer.status, EqualizerStatus::Configured);
    assert_eq!(view.loudness.status, LoudnessStatus::Configured);
    (f, view)
}

#[tokio::test]
async fn simultaneous_poll_faults_remove_both_filters_without_reloading_or_ending_the_song() {
    let (f, before) = loaded().await;
    f.device.state.lock().unwrap().view.position = Some(8.0);
    f.device
        .optional_poll_faults
        .store(POLL_FAULT_EQUALIZER | POLL_FAULT_LOUDNESS, Ordering::SeqCst);
    let view = until(&f.player, |view| {
        view.state == PlaybackStatus::Error
            || (view.equalizer.status == EqualizerStatus::Failed
                && view.loudness.status == LoudnessStatus::Failed)
    })
    .await;
    assert_eq!(view.state, PlaybackStatus::Playing);
    assert_eq!(view.play_id, before.play_id);
    assert_eq!(view.position, Some(8.0));
    assert!(view.error.is_none());
    assert_eq!(view.equalizer.preferences, before.equalizer.preferences);
    assert_eq!(view.loudness.preferences, before.loudness.preferences);
    assert!(f.device.equalizer.lock().unwrap().is_none());
    assert!(f.device.loudness.lock().unwrap().is_none());
    let native = f.device.state.lock().unwrap();
    assert_eq!(native.loads, 1);
    assert!(!native.view.paused);
}

#[tokio::test]
async fn a_fault_first_reported_during_pause_preserves_the_new_transport_and_other_effect() {
    for fault in [POLL_FAULT_EQUALIZER, POLL_FAULT_LOUDNESS] {
        let (f, before) = loaded().await;
        f.device
            .optional_faults_after_pause
            .store(fault, Ordering::SeqCst);
        let view = f.command(Cmd::Pause(true)).await.unwrap();
        assert_eq!(view.state, PlaybackStatus::Paused);
        assert_eq!(view.play_id, before.play_id);
        assert!(view.error.is_none());
        assert_eq!(
            view.equalizer.status == EqualizerStatus::Failed,
            fault == POLL_FAULT_EQUALIZER
        );
        assert_eq!(
            view.loudness.status == LoudnessStatus::Failed,
            fault == POLL_FAULT_LOUDNESS
        );
        assert_eq!(
            f.device.equalizer.lock().unwrap().is_none(),
            fault == POLL_FAULT_EQUALIZER
        );
        assert_eq!(
            f.device.loudness.lock().unwrap().is_none(),
            fault == POLL_FAULT_LOUDNESS
        );
        let resumed = f.command(Cmd::Pause(false)).await.unwrap();
        assert_eq!(resumed.state, PlaybackStatus::Playing);
        assert_eq!(resumed.play_id, before.play_id);
        assert_eq!(f.device.state.lock().unwrap().loads, 1);
    }
}

#[tokio::test]
async fn an_unconfirmed_filter_detach_stops_instead_of_publishing_the_old_pause_state() {
    for fault in [POLL_FAULT_EQUALIZER, POLL_FAULT_LOUDNESS] {
        let (f, _) = loaded().await;
        f.device
            .equalizer_clear_fail
            .store(fault == POLL_FAULT_EQUALIZER, Ordering::SeqCst);
        f.device
            .loudness_clear_fail
            .store(fault == POLL_FAULT_LOUDNESS, Ordering::SeqCst);
        f.device
            .optional_faults_after_pause
            .store(fault, Ordering::SeqCst);
        assert!(f.command(Cmd::Pause(true)).await.is_err());
        let view = f.command(Cmd::Snapshot).await.unwrap();
        assert_eq!(view.state, PlaybackStatus::Error);
        assert!(view.play_id.is_none());
        assert!(view.error.is_some());
        assert_eq!(
            view.equalizer.status == EqualizerStatus::Failed,
            fault == POLL_FAULT_EQUALIZER
        );
        assert_eq!(
            view.loudness.status == LoudnessStatus::Failed,
            fault == POLL_FAULT_LOUDNESS
        );
        let native = f.device.state.lock().unwrap();
        assert_eq!(native.loads, 1);
        assert!(native.view.entry_id.is_none() && native.view.paused);
    }
}
