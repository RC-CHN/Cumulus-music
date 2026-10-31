#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::playback::controller::PlaybackCommand as Cmd;
use serde_json::json;
use std::sync::atomic::Ordering;
use support::*;

#[tokio::test]
async fn zero_volume_restores_the_last_nonzero_value_after_player_recreation() {
    let f = Fixture::new();
    f.command(replace(&[1])).await.unwrap();
    f.command(Cmd::Pause(true)).await.unwrap();
    let original = f.command(Cmd::Volume(31.25)).await.unwrap();
    let muted = f.command(Cmd::Volume(0.0)).await.unwrap();
    assert_eq!(muted.volume, 0.0);
    assert_eq!(muted.unmute_volume, 31.25);
    assert_eq!(muted.play_id, original.play_id);
    assert_eq!(muted.fence(), original.fence());
    let reopened = f.another_player();
    let restored = reopened.command(&f.token(), Cmd::Restore).await.unwrap();
    assert_eq!(restored.volume, 0.0);
    assert_eq!(restored.unmute_volume, 31.25);
    assert_eq!(
        reopened
            .command(&f.token(), Cmd::Volume(restored.unmute_volume))
            .await
            .unwrap()
            .volume,
        31.25
    );
    assert_eq!(
        f.store
            .get("9", "playback", "preferences")
            .unwrap()
            .unwrap()["unmute_volume"],
        31.25
    );
}

#[tokio::test]
async fn legacy_muted_preferences_have_the_official_ten_percent_fallback() {
    let f = Fixture::new();
    let old = json!({"volume":0.0,"speed":1.25});
    f.store.put("9", "playback", "preferences", &old).unwrap();
    let reopened = f.another_player();
    let view = reopened.command(&f.token(), Cmd::Snapshot).await.unwrap();
    assert_eq!(view.volume, 0.0);
    assert_eq!(view.speed, 1.25);
    assert_eq!(view.unmute_volume, 10.0);
    assert_eq!(
        f.store
            .get("9", "playback", "preferences")
            .unwrap()
            .unwrap(),
        old
    );
    let unmuted = reopened
        .command(&f.token(), Cmd::Volume(view.unmute_volume))
        .await
        .unwrap();
    assert_eq!(unmuted.volume, 10.0);
}

#[tokio::test]
async fn failed_persistence_rolls_back_both_actual_and_restore_volume() {
    let f = Fixture::new();
    f.command(replace(&[1])).await.unwrap();
    f.command(Cmd::Volume(37.0)).await.unwrap();
    let before = f.store.get("9", "playback", "preferences").unwrap();
    f.sql("CREATE TRIGGER deny_volume BEFORE UPDATE ON state WHEN NEW.kind='playback' AND NEW.key='preferences' BEGIN SELECT RAISE(ABORT,'synthetic volume disk failure'); END;");
    assert!(f.command(Cmd::Volume(0.0)).await.is_err());
    assert!(f.command(Cmd::Volume(80.0)).await.is_err());
    let kept = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(kept.volume, 37.0);
    assert_eq!(kept.unmute_volume, 37.0);
    assert_eq!(f.device.state.lock().unwrap().view.volume, Some(37.0));
    assert_eq!(f.store.get("9", "playback", "preferences").unwrap(), before);
}

#[tokio::test]
async fn mute_restore_is_isolated_between_accounts() {
    let f = Fixture::new();
    f.command(Cmd::Volume(73.0)).await.unwrap();
    f.command(Cmd::Volume(0.0)).await.unwrap();
    f.session
        .adopt(f.api.clone(), json!({"userId":10}), &f.token())
        .unwrap();
    let other = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(other.volume, 100.0);
    assert_eq!(other.unmute_volume, 10.0);
    f.command(Cmd::Volume(23.0)).await.unwrap();
    f.session
        .adopt(f.api.clone(), json!({"userId":9}), &f.token())
        .unwrap();
    let restored = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(restored.volume, 0.0);
    assert_eq!(restored.unmute_volume, 73.0);
}

#[tokio::test]
async fn muting_during_source_loading_preserves_the_pending_playback() {
    let f = Fixture::new();
    f.api.hold.store(1, Ordering::SeqCst);
    let playing = f.start(replace(&[1]));
    tokio::time::timeout(std::time::Duration::from_secs(2), f.api.started.notified())
        .await
        .unwrap();
    f.command(Cmd::Volume(42.0)).await.unwrap();
    let muted = f.command(Cmd::Volume(0.0)).await.unwrap();
    f.api.release.notify_one();
    let loaded = playing.await.unwrap().unwrap();
    assert_eq!(loaded.volume, 0.0);
    assert_eq!(loaded.unmute_volume, 42.0);
    assert_eq!(loaded.fence(), muted.fence());
    assert_eq!(f.device.state.lock().unwrap().view.volume, Some(0.0));
}
