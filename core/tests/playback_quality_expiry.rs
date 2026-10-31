#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::{
    membership::Quality,
    playback::{
        controller::{PlaybackCommand as Cmd, PlaybackStatus as Status},
        quality::{QualityRight, trials::QualityTrials},
        quality_preferences::QualityPreferences,
    },
};
use std::{sync::atomic::Ordering, time::Duration};
use support::*;
async fn bind(f: &Fixture, right: QualityRight) -> String {
    f.api.quality_use_state.store(1, Ordering::SeqCst);
    let lease = QualityTrials::new(f.session.clone())
        .unwrap()
        .adopt_for(&f.token(), right)
        .await
        .unwrap();
    f.command(Cmd::TrialQuality {
        right,
        operation: lease.operation.clone(),
    })
    .await
    .unwrap();
    lease.operation
}
#[tokio::test]
async fn expiry_restores_the_exact_previous_preference_without_resuming_or_reloading_audio() {
    let f = Fixture::persistent();
    f.command(replace(&[1, 2])).await.unwrap();
    f.command(Cmd::Pause(true)).await.unwrap();
    f.command(Cmd::Seek(42.5)).await.unwrap();
    f.command(Cmd::Quality(Quality::Lossless)).await.unwrap();
    let operation = bind(&f, QualityRight::JyMaster).await;
    let before = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(
        before
            .quality_preferences
            .preferences
            .trial
            .as_ref()
            .unwrap()
            .operation,
        operation
    );
    assert_eq!(
        before
            .quality_preferences
            .preferences
            .trial
            .as_ref()
            .unwrap()
            .previous,
        Quality::Lossless
    );
    let loads = f.device.state.lock().unwrap().loads;
    f.api.quality_use_state.store(2, Ordering::SeqCst);
    let restored = until(&f.player, |s| {
        s.quality_preferences.preferences.quality == Quality::Lossless
    })
    .await;
    assert!(restored.quality_preferences.preferences.trial.is_none());
    assert_ne!(
        restored.quality_preferences.preferences.revision,
        before.quality_preferences.preferences.revision
    );
    assert_eq!(restored.state, Status::Paused);
    assert_eq!(restored.position, Some(42.5));
    assert_eq!(restored.play_id, before.play_id);
    assert_eq!(restored.requested_quality.as_deref(), Some("jymaster"));
    assert_eq!(f.device.state.lock().unwrap().loads, loads);
    f.command(Cmd::Next).await.unwrap();
    assert!(
        f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(id, q)| *id == 2 && q == "lossless")
    );
}
#[tokio::test]
async fn a_later_manual_quality_choice_cannot_be_undone_by_expiry() {
    let f = Fixture::persistent();
    f.command(replace(&[1, 2])).await.unwrap();
    bind(&f, QualityRight::JyMaster).await;
    let manual = f
        .command(Cmd::Quality(Quality::HiRes))
        .await
        .unwrap()
        .quality_preferences
        .preferences;
    assert!(manual.trial.is_none());
    f.api.quality_use_state.store(2, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(1350)).await;
    assert_eq!(
        f.command(Cmd::Snapshot)
            .await
            .unwrap()
            .quality_preferences
            .preferences,
        manual
    );
}
#[tokio::test]
async fn switching_between_trials_keeps_the_original_nontrial_fallback() {
    let f = Fixture::persistent();
    f.command(replace(&[1])).await.unwrap();
    f.command(Cmd::Quality(Quality::Lossless)).await.unwrap();
    bind(&f, QualityRight::JyMaster).await;
    bind(&f, QualityRight::Sky).await;
    let trial = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(
        trial
            .quality_preferences
            .preferences
            .trial
            .as_ref()
            .unwrap()
            .previous,
        Quality::Lossless
    );
    f.api.quality_use_state.store(2, Ordering::SeqCst);
    let restored = until(&f.player, |s| {
        s.quality_preferences.preferences.quality == Quality::Lossless
    })
    .await;
    assert!(restored.quality_preferences.preferences.trial.is_none());
}
#[tokio::test]
async fn failed_rights_reads_preserve_the_bound_preference_instead_of_claiming_expiry() {
    let f = Fixture::persistent();
    f.command(replace(&[1])).await.unwrap();
    bind(&f, QualityRight::JyMaster).await;
    let trial = f
        .command(Cmd::Snapshot)
        .await
        .unwrap()
        .quality_preferences
        .preferences;
    f.api.quality_read_failed.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(1350)).await;
    let kept = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(kept.quality_preferences.preferences, trial);
    assert!(kept.error.is_none());
    assert_eq!(kept.state, Status::Playing);
}
#[tokio::test]
async fn restored_trial_preferences_are_checked_by_a_new_player_even_when_idle() {
    let mut f = Fixture::persistent();
    f.command(replace(&[1])).await.unwrap();
    f.command(Cmd::Quality(Quality::Lossless)).await.unwrap();
    bind(&f, QualityRight::JyMaster).await;
    f.command(Cmd::Stop).await.unwrap();
    let reopen = f.another_player();
    let old = std::mem::replace(&mut f.player, reopen.clone());
    drop(old);
    f.api.quality_use_state.store(2, Ordering::SeqCst);
    let restored = until(&reopen, |s| {
        s.quality_preferences.preferences.quality == Quality::Lossless
    })
    .await;
    assert_eq!(restored.state, Status::Idle);
    assert!(restored.quality_preferences.preferences.trial.is_none());
    assert_eq!(
        QualityPreferences::read(&f.store, "9").unwrap().quality,
        Quality::Lossless
    );
}
#[tokio::test]
async fn an_expired_trial_during_source_authorization_cannot_commit_over_the_restored_preference() {
    let f = Fixture::persistent();
    f.command(replace(&[1])).await.unwrap();
    f.command(Cmd::Pause(true)).await.unwrap();
    f.api.quality_use_state.store(1, Ordering::SeqCst);
    let trials = QualityTrials::new(f.session.clone()).unwrap();
    let lease = trials
        .adopt_for(&f.token(), QualityRight::JyMaster)
        .await
        .unwrap();
    f.api.hold.store(1, Ordering::SeqCst);
    let pending = f.start(Cmd::TrialQuality {
        right: QualityRight::JyMaster,
        operation: lease.operation,
    });
    f.api.started.notified().await;
    f.api.quality_use_state.store(2, Ordering::SeqCst);
    trials
        .observe_for(&f.token(), QualityRight::JyMaster)
        .await
        .unwrap();
    f.api.release.notify_one();
    assert!(pending.await.unwrap().is_err());
    let kept = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(kept.state, Status::Paused);
    assert!(kept.quality_preferences.preferences.trial.is_none());
    assert_eq!(
        kept.quality_preferences.preferences.quality,
        Quality::Standard
    );
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
}
