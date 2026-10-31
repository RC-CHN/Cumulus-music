#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::{
    Error,
    membership::Quality,
    playback::{
        PlaybackMode, PlaybackQueue, TimelineMode,
        controller::{PlaybackCommand as Cmd, PlaybackStatus as Status},
        engine::EndReason,
    },
};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use support::*;

#[tokio::test]
async fn controls_persist_and_reopen_paused_with_the_same_entry_and_position() {
    let f = Fixture::new();
    let playing = f.command(replace(&[1, 2, 1])).await.unwrap();
    assert_eq!(playing.state, Status::Playing);
    assert_eq!(playing.queue_len, 3);
    f.command(Cmd::Seek(42.5)).await.unwrap();
    f.command(Cmd::Pause(true)).await.unwrap();
    f.command(Cmd::Volume(37.0)).await.unwrap();
    f.command(Cmd::Speed(1.25)).await.unwrap();
    f.command(Cmd::Mode(PlaybackMode::Random)).await.unwrap();
    let queue = f.player.queue(&f.token()).await.unwrap();
    assert_ne!(queue.items[0].entry_id, queue.items[2].entry_id);
    let restarted = f.another_player();
    let view = restarted.command(&f.token(), Cmd::Restore).await.unwrap();
    assert_eq!(view.state, Status::Paused);
    assert_eq!(view.position, Some(42.5));
    assert_eq!(view.volume, 37.0);
    assert_eq!(view.speed, 1.25);
    assert_eq!(view.mode, PlaybackMode::Random);
    assert_eq!(restarted.queue(&f.token()).await.unwrap(), queue);
    assert!(matches!(
        root(&f.command(Cmd::Seek(f64::NAN)).await.err().unwrap()),
        Error::InvalidInput(_)
    ));
    assert!(f.command(Cmd::Volume(101.0)).await.is_err());
    assert_eq!(
        f.command(Cmd::Snapshot).await.unwrap().state,
        Status::Paused
    );
    f.command(Cmd::Stop).await.unwrap();
    restarted.command(&f.token(), Cmd::Stop).await.unwrap();
}

#[tokio::test]
async fn paused_seek_survives_latency_reports_speed_changes_and_repeated_pause() {
    let f = Fixture::new();
    f.command(replace(&[1, 2])).await.unwrap();
    f.command(Cmd::Pause(true)).await.unwrap();
    f.command(Cmd::Seek(42.5)).await.unwrap();
    // Model libmpv's real paused/null output latency. It reports a decoder
    // position before the accepted seek and recomputes that latency on speed.
    f.device.state.lock().unwrap().view.position = Some(42.1);
    tokio::time::sleep(std::time::Duration::from_millis(450)).await;
    assert_eq!(f.command(Cmd::Snapshot).await.unwrap().position, Some(42.5));
    f.command(Cmd::Speed(1.25)).await.unwrap();
    f.device.state.lock().unwrap().view.position = Some(41.9);
    tokio::time::sleep(std::time::Duration::from_millis(450)).await;
    let paused = f.command(Cmd::Pause(true)).await.unwrap();
    assert_eq!(paused.state, Status::Paused);
    assert_eq!(paused.position, Some(42.5));
    f.command(Cmd::Volume(37.0)).await.unwrap();
    let restored = f.another_player();
    let view = restored.command(&f.token(), Cmd::Restore).await.unwrap();
    assert_eq!(view.position, Some(42.5));
    assert_eq!(view.speed, 1.25);
    restored.command(&f.token(), Cmd::Stop).await.unwrap();
    f.command(Cmd::Pause(false)).await.unwrap();
    f.device.state.lock().unwrap().view.position = Some(43.0);
    until(&f.player, |s| s.position == Some(43.0)).await;
    f.command(Cmd::Stop).await.unwrap();
}

#[tokio::test]
async fn eof_is_bound_to_native_entry_and_single_loop_differs_from_manual_next() {
    let f = Fixture::new();
    f.command(replace(&[1, 2])).await.unwrap();
    f.command(Cmd::Mode(PlaybackMode::SingleLoop))
        .await
        .unwrap();
    event(&f.device, 999, EndReason::Eof);
    // Waiting through multiple polls verifies that a stale EOF cannot advance.
    tokio::time::sleep(std::time::Duration::from_millis(450)).await;
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    let first = f.player.snapshot().play_id;
    event(&f.device, 1, EndReason::Eof);
    until(&f.player, |s| {
        s.state == Status::Playing && s.play_id != first
    })
    .await;
    assert_eq!(song(&f.player.snapshot()), Some(1));
    let second = f.command(Cmd::Next).await.unwrap();
    assert_eq!(song(&second), Some(2));
    f.command(Cmd::Mode(PlaybackMode::Sequential))
        .await
        .unwrap();
    let native = f.device.state.lock().unwrap().view.entry_id.unwrap();
    event(&f.device, native, EndReason::Eof);
    let ended = until(&f.player, |s| s.state == Status::Ended).await;
    assert_eq!(ended.position, Some(120.0));
    let restored = f.command(Cmd::Restore).await.unwrap();
    assert_eq!(restored.position, Some(0.0));
    assert_eq!(restored.state, Status::Paused);
}

#[tokio::test]
async fn rapid_replacement_cancels_resolving_owner_and_cannot_publish_old_song() {
    let f = Fixture::new();
    f.api.hold.store(1, Ordering::SeqCst);
    let old = f.start(replace(&[1]));
    f.api.started.notified().await;
    let next = f.command(replace(&[2])).await.unwrap();
    assert_eq!(song(&next), Some(2));
    assert!(matches!(
        root(&old.await.unwrap().err().unwrap()),
        Error::Cancelled
    ));
    f.api.release.notify_waiters();
    assert_eq!(song(&f.command(Cmd::Snapshot).await.unwrap()), Some(2));
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
}

#[tokio::test]
async fn clearing_a_queue_during_decode_cancels_before_unpause() {
    let f = Fixture::new();
    f.device.hold_load.store(true, Ordering::SeqCst);
    let load = f.start(replace(&[1]));
    f.device.started.notified().await;
    let cleared = f.command(replace(&[])).await.unwrap();
    assert_eq!(cleared.state, Status::Idle);
    assert_eq!(cleared.queue_len, 0);
    assert!(matches!(
        root(&load.await.unwrap().err().unwrap()),
        Error::Cancelled
    ));
    f.device.hold_load.store(false, Ordering::SeqCst);
    assert_eq!(f.device.state.lock().unwrap().loads, 0);
    assert!(f.store.get("9", "playback", "position").unwrap().is_none());
}

#[tokio::test]
async fn caller_abort_stops_pending_decode_and_wakes_the_actor() {
    let f = Fixture::new();
    f.device.hold_load.store(true, Ordering::SeqCst);
    let load = f.start(replace(&[1]));
    f.device.started.notified().await;
    load.abort();
    assert!(load.await.err().unwrap().is_cancelled());
    until(&f.player, |s| s.state == Status::Idle).await;
    f.device.hold_load.store(false, Ordering::SeqCst);
    assert_eq!(f.device.state.lock().unwrap().loads, 0);
    f.command(Cmd::Play).await.unwrap();
}

#[tokio::test]
async fn account_change_clears_old_private_state_even_when_new_storage_is_corrupt() {
    let f = Fixture::new();
    f.command(replace(&[1])).await.unwrap();
    let old = f.token();
    f.store
        .put(
            "10",
            "playback",
            "queue",
            &json!({"items":[],"current":"invalid"}),
        )
        .unwrap();
    f.session
        .adopt(f.api.clone(), json!({"userId":10}), &old)
        .unwrap();
    let view = until(&f.player, |s| {
        s.account.uid.as_deref() == Some("10") && s.state == Status::Error
    })
    .await;
    assert!(view.selected.is_none());
    assert_eq!(view.queue_len, 0);
    assert_eq!(view.volume, 100.0);
    assert!(f.device.state.lock().unwrap().view.entry_id.is_none());
    f.store.delete("10", "playback", "queue").unwrap();
    assert!(matches!(
        f.player.command(&old, Cmd::Play).await.err().unwrap(),
        Error::StaleResponse
    ));
    assert!(matches!(
        root(&f.command(Cmd::Play).await.err().unwrap()),
        Error::State("queue is empty")
    ));
    f.command(replace(&[2])).await.unwrap();
    assert_eq!(
        f.store.get("9", "playback", "queue").unwrap().unwrap()["items"][0]["resource"]["id"],
        1
    );
}

#[tokio::test]
async fn selected_quality_is_used_by_existing_queue_next_song_and_survives_reopen() {
    let f = Fixture::new();
    f.command(replace(&[1, 2, 3])).await.unwrap();
    f.command(Cmd::Quality(Quality::Lossless)).await.unwrap();
    let next = f.command(Cmd::Next).await.unwrap();
    assert_eq!(next.requested_quality.as_deref(), Some("lossless"));
    assert!(
        f.api
            .calls
            .lock()
            .unwrap()
            .contains(&(2, "lossless".into()))
    );
    f.command(Cmd::Stop).await.unwrap();
    let reopened = f.another_player();
    reopened.command(&f.token(), Cmd::Restore).await.unwrap();
    let next = reopened.command(&f.token(), Cmd::Next).await.unwrap();
    assert_eq!(next.requested_quality.as_deref(), Some("lossless"));
    assert!(
        f.api
            .calls
            .lock()
            .unwrap()
            .contains(&(3, "lossless".into()))
    );
    reopened.command(&f.token(), Cmd::Stop).await.unwrap();
}

#[tokio::test]
async fn implicit_native_queue_quality_and_null_voice_metadata_use_the_song_preference() {
    let f = Fixture::new();
    f.command(Cmd::Replace {
        resources: vec![json!({"id":1,"voice_id":null})],
        index: 0,
        source: Value::Null,
    })
    .await
    .unwrap();
    f.command(Cmd::Quality(Quality::Lossless)).await.unwrap();
    let selected = f
        .command(Cmd::Replace {
            resources: vec![json!({"id":2,"voice_id":null})],
            index: 0,
            source: Value::Null,
        })
        .await
        .unwrap();
    assert!(selected.quality_preferences_apply);
    assert_eq!(selected.requested_quality.as_deref(), Some("lossless"));
    f.command(Cmd::Append {
        resources: vec![json!({"id":3,"level":null,"voice_id":null})],
        source: Value::Null,
    })
    .await
    .unwrap();
    assert_eq!(
        f.command(Cmd::Next)
            .await
            .unwrap()
            .requested_quality
            .as_deref(),
        Some("lossless")
    );
    f.command(Cmd::Stop).await.unwrap();
}

#[tokio::test]
async fn denied_quality_preserves_playback_and_authorized_downgrade_preserves_clock() {
    let f = Fixture::new();
    let first = f.command(replace(&[1])).await.unwrap();
    f.command(Cmd::Seek(47.0)).await.unwrap();
    f.command(Cmd::Pause(true)).await.unwrap();
    f.api.deny_lossless.store(true, Ordering::SeqCst);
    assert!(
        f.command(Cmd::Quality(Quality::Lossless))
            .await
            .err()
            .unwrap()
            .is_service_code(403)
    );
    let denied = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(denied.play_id, first.play_id);
    assert_eq!(
        denied.quality_preferences.preferences.quality,
        Quality::Standard
    );
    assert_eq!(denied.state, Status::Paused);
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    f.api.deny_lossless.store(false, Ordering::SeqCst);
    let changed = f.command(Cmd::Quality(Quality::Lossless)).await.unwrap();
    assert_eq!(changed.requested_quality.as_deref(), Some("lossless"));
    assert_eq!(changed.actual_quality.as_deref(), Some("standard"));
    assert_eq!(
        changed.quality_preferences.preferences.quality,
        Quality::Lossless
    );
    assert_eq!(changed.position, Some(47.0));
    assert_eq!(changed.state, Status::Paused);
    let count = f.api.count(1);
    let refreshed = f.command(Cmd::RefreshSource).await.unwrap();
    assert!(f.api.count(1) > count);
    assert_eq!(refreshed.position, Some(47.0));
}

#[tokio::test]
async fn explicit_new_queue_keeps_its_quality_until_the_preference_changes_again() {
    let f = Fixture::new();
    f.command(replace(&[1, 2])).await.unwrap();
    f.command(Cmd::Quality(Quality::Lossless)).await.unwrap();
    let explicit = Cmd::Replace {
        resources: vec![
            json!({"id":3,"level":"exhigh"}),
            json!({"id":4,"level":"exhigh"}),
        ],
        index: 0,
        source: Value::Null,
    };
    let first = f.command(explicit).await.unwrap();
    assert_eq!(first.requested_quality.as_deref(), Some("exhigh"));
    assert_eq!(
        first.quality_preferences.preferences.quality,
        Quality::Lossless
    );
    assert_eq!(
        f.command(Cmd::Next)
            .await
            .unwrap()
            .requested_quality
            .as_deref(),
        Some("exhigh")
    );
    f.command(Cmd::Quality(Quality::HiRes)).await.unwrap();
    assert_eq!(
        f.command(Cmd::Previous)
            .await
            .unwrap()
            .requested_quality
            .as_deref(),
        Some("hires")
    );
    f.command(Cmd::Stop).await.unwrap();
}

#[tokio::test]
async fn quality_preferences_and_queue_inheritance_are_isolated_between_accounts() {
    let f = Fixture::new();
    f.command(replace(&[1, 2])).await.unwrap();
    f.command(Cmd::Quality(Quality::Lossless)).await.unwrap();
    f.session
        .adopt(f.api.clone(), json!({"userId":10}), &f.token())
        .unwrap();
    let other = f.command(replace(&[3, 4])).await.unwrap();
    assert_eq!(
        other.quality_preferences.preferences.quality,
        Quality::Standard
    );
    f.command(Cmd::Quality(Quality::ExHigh)).await.unwrap();
    f.session
        .adopt(f.api.clone(), json!({"userId":9}), &f.token())
        .unwrap();
    let restored = f.command(Cmd::Restore).await.unwrap();
    assert_eq!(
        restored.quality_preferences.preferences.quality,
        Quality::Lossless
    );
    let next = f.command(Cmd::Next).await.unwrap();
    assert_eq!(song(&next), Some(2));
    assert_eq!(next.requested_quality.as_deref(), Some("lossless"));
    f.command(Cmd::Stop).await.unwrap();
}

#[tokio::test]
async fn another_hosts_quality_change_rejects_old_authorization_without_interrupting_audio() {
    use cumulus_core::playback::quality_preferences::QualityPreferences;
    let f = Fixture::new();
    let first = f.command(replace(&[1, 2])).await.unwrap();
    f.api.hold.store(1, Ordering::SeqCst);
    let pending = f.start(Cmd::Quality(Quality::Lossless));
    f.api.started.notified().await;
    QualityPreferences::replace(&f.store, "9", "initial", Quality::HiRes).unwrap();
    f.api.release.notify_waiters();
    assert!(pending.await.unwrap().is_err());
    let kept = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(kept.play_id, first.play_id);
    assert_eq!(kept.quality_preferences.preferences.quality, Quality::HiRes);
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    f.api.hold.store(0, Ordering::SeqCst);
    assert_eq!(
        f.command(Cmd::Next)
            .await
            .unwrap()
            .requested_quality
            .as_deref(),
        Some("hires")
    );
    f.command(Cmd::Stop).await.unwrap();
}

#[tokio::test]
async fn damaged_quality_preferences_allow_playback_and_require_explicit_non_reusing_repair() {
    use cumulus_core::playback::quality_preferences::QualityPreferences;
    let f = Fixture::new();
    f.command(replace(&[1, 2])).await.unwrap();
    let saved = f.command(Cmd::Quality(Quality::Lossless)).await.unwrap();
    let old_revision = saved.quality_preferences.preferences.revision;
    f.sql("UPDATE state SET value='malformed' WHERE owner='9' AND kind='playback-quality-v1' AND key='current'");
    let reopened = f.another_player();
    let restored = reopened.command(&f.token(), Cmd::Restore).await.unwrap();
    assert!(restored.quality_preferences.damaged);
    assert_eq!(restored.state, Status::Paused);
    assert!(
        reopened
            .command(&f.token(), Cmd::Quality(Quality::HiRes))
            .await
            .is_err()
    );
    let repaired = reopened
        .command(&f.token(), Cmd::RepairQuality)
        .await
        .unwrap();
    assert!(!repaired.quality_preferences.damaged);
    assert_ne!(
        repaired.quality_preferences.preferences.revision,
        old_revision
    );
    assert!(QualityPreferences::replace(&f.store, "9", &old_revision, Quality::HiRes).is_err());
    assert_eq!(
        f.store
            .values("9", "playback-quality-recovery-v1")
            .unwrap()
            .len(),
        1
    );
    let next = reopened.command(&f.token(), Cmd::Next).await.unwrap();
    assert_eq!(next.requested_quality.as_deref(), Some("standard"));
    reopened.command(&f.token(), Cmd::Stop).await.unwrap();
    f.command(Cmd::Stop).await.unwrap();
}

#[tokio::test]
async fn a_quality_storage_failure_preserves_audio_and_rolls_back_the_preference() {
    let f = Fixture::new();
    let before = f.command(replace(&[1, 2])).await.unwrap();
    f.sql("CREATE TRIGGER deny_quality BEFORE INSERT ON state WHEN NEW.kind='playback-quality-v1' BEGIN SELECT RAISE(FAIL,'injected'); END;");
    assert!(f.command(Cmd::Quality(Quality::Lossless)).await.is_err());
    let after = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(after.play_id, before.play_id);
    assert_eq!(after.quality_preferences.preferences.revision, "initial");
    assert!(
        f.store
            .values("9", "playback-quality-v1")
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    f.command(Cmd::Stop).await.unwrap();
}

#[tokio::test]
async fn queue_write_failure_also_rolls_back_the_new_quality_preference() {
    let f = Fixture::new();
    let before = f.command(replace(&[1, 2])).await.unwrap();
    f.sql("CREATE TRIGGER deny_quality_queue BEFORE INSERT ON state WHEN NEW.kind='playback' AND NEW.key='queue' BEGIN SELECT RAISE(FAIL,'injected'); END;");
    assert!(f.command(Cmd::Quality(Quality::Lossless)).await.is_err());
    let after = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(after.play_id, before.play_id);
    assert_eq!(after.quality_preferences.preferences.revision, "initial");
    assert!(
        f.store
            .values("9", "playback-quality-v1")
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    f.command(Cmd::Stop).await.unwrap();
}

#[test]
fn missing_quality_record_is_not_treated_as_a_first_run_and_repair_keeps_the_saved_marker() {
    use cumulus_core::playback::quality_preferences::QualityPreferences;
    let f = tempfile::tempdir().unwrap();
    let store = cumulus_core::storage::StateStore::open(f.path()).unwrap();
    let saved = QualityPreferences::replace(&store, "9", "initial", Quality::Lossless).unwrap();
    store.delete("9", "playback-quality-v1", "current").unwrap();
    assert!(QualityPreferences::read(&store, "9").is_err());
    assert!(QualityPreferences::replace(&store, "9", "initial", Quality::HiRes).is_err());
    let repaired = QualityPreferences::repair(&store, "9").unwrap();
    assert_ne!(repaired.revision, saved.revision);
    assert_eq!(repaired.quality, Quality::Standard);
    assert!(QualityPreferences::repair(&store, "9").is_err());
    let archived = store.values("9", "playback-quality-recovery-v1").unwrap();
    assert_eq!(archived.len(), 1);
    let raw = archived.values().next().unwrap();
    assert_eq!(raw["saved_raw"], "true");
    assert!(raw["current_raw"].is_null());
}

#[tokio::test]
async fn preview_bounds_and_legacy_position_restore_use_the_authorized_clock() {
    let f = Fixture::new();
    f.api.trial.store(true, Ordering::SeqCst);
    f.device.state.lock().unwrap().duration = 20.0;
    let first = f.command(replace(&[1])).await.unwrap();
    assert_eq!(first.position, Some(30.0));
    let timeline = first.timeline.unwrap();
    assert_eq!(timeline.mode, TimelineMode::Trial);
    assert_eq!(timeline.end, Some(50.0));
    assert_eq!(f.device.state.lock().unwrap().bounds[0], (0.0, Some(30.0)));
    assert!(f.command(Cmd::Seek(25.0)).await.is_err());
    assert!(f.command(Cmd::Seek(55.0)).await.is_err());
    f.command(Cmd::Seek(38.0)).await.unwrap();
    assert_eq!(f.device.state.lock().unwrap().view.position, Some(8.0));
    f.command(Cmd::Stop).await.unwrap();
    let id = first.selected.unwrap().entry_id;
    f.store
        .put(
            "9",
            "playback",
            "position",
            &json!({"entry_id":id,"seconds":8.0}),
        )
        .unwrap();
    let restored = f.command(Cmd::Restore).await.unwrap();
    assert_eq!(restored.position, Some(38.0));
    assert_eq!(restored.state, Status::Paused);
    f.command(Cmd::Stop).await.unwrap();
    f.store
        .put(
            "9",
            "playback",
            "position",
            &json!({"entry_id":id,"seconds":90.0,"timeline_version":1}),
        )
        .unwrap();
    let reset = f.command(Cmd::Restore).await.unwrap();
    assert!(reset.position_reset);
    assert_eq!(reset.position, Some(30.0));
}

#[tokio::test]
async fn database_failure_rolls_back_preferences_and_stops_unpersisted_transport() {
    let f = Fixture::new();
    f.command(replace(&[1])).await.unwrap();
    f.command(Cmd::Volume(20.0)).await.unwrap();
    f.sql("CREATE TRIGGER deny_preferences BEFORE UPDATE ON state WHEN NEW.key='preferences' BEGIN SELECT RAISE(FAIL,'fixture'); END;");
    assert!(matches!(
        root(&f.command(Cmd::Volume(40.0)).await.err().unwrap()),
        Error::Storage(_)
    ));
    assert_eq!(f.device.state.lock().unwrap().view.volume, Some(20.0));
    assert_eq!(f.command(Cmd::Snapshot).await.unwrap().volume, 20.0);
    f.sql("CREATE TRIGGER deny_position BEFORE UPDATE ON state WHEN NEW.key='position' BEGIN SELECT RAISE(FAIL,'fixture'); END;");
    assert!(matches!(
        root(&f.command(Cmd::Seek(25.0)).await.err().unwrap()),
        Error::Storage(_)
    ));
    let view = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(view.state, Status::Error);
    assert!(f.device.state.lock().unwrap().view.entry_id.is_none());
    assert_eq!(
        f.store.get("9", "playback", "position").unwrap().unwrap()["seconds"],
        0.0
    );
}

#[tokio::test]
async fn external_queue_edits_rebind_same_entry_and_never_resurrect_removed_position() {
    let f = Fixture::new();
    f.command(replace(&[1])).await.unwrap();
    f.command(Cmd::Seek(12.0)).await.unwrap();
    f.command(Cmd::Pause(true)).await.unwrap();
    let mut other = PlaybackQueue::new(Some(f.store.clone()), "9").unwrap();
    let id = other.selected().unwrap().entry_id;
    other
        .update_resource(&id, json!({"id":2,"dt":120000}))
        .unwrap();
    let changed = until(&f.player, |s| {
        s.state == Status::Paused && song(s) == Some(2)
    })
    .await;
    assert_eq!(changed.position, Some(12.0));
    other.remove(&[id]).unwrap();
    f.store.delete("9", "playback", "position").unwrap();
    until(&f.player, |s| s.state == Status::Idle && s.queue_len == 0).await;
    assert!(f.store.get("9", "playback", "position").unwrap().is_none());
    assert!(f.device.state.lock().unwrap().view.entry_id.is_none());
}

#[tokio::test]
async fn prefetch_refreshes_expired_source_without_changing_queue_or_audio() {
    let f = Fixture::new();
    f.command(replace(&[1, 2])).await.unwrap();
    until(&f.player, |s| s.prefetch.state == "ready").await;
    let before = f.player.queue(&f.token()).await.unwrap();
    assert_eq!(f.api.count(2), 1);
    f.clock.0.store(6, Ordering::SeqCst);
    until(&f.player, |s| {
        s.prefetch.state == "ready" && f.api.count(2) == 2
    })
    .await;
    assert_eq!(f.player.queue(&f.token()).await.unwrap(), before);
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    let next = f.command(Cmd::Next).await.unwrap();
    assert_eq!(song(&next), Some(2));
    assert_eq!(f.api.count(2), 2);
    assert!(
        f.store
            .get("9", "playback", "queue")
            .unwrap()
            .unwrap()
            .to_string()
            .find("secret=")
            .is_none()
    );
    assert_eq!(
        f.store.get("9", "playback", "position").unwrap().unwrap()["timeline_version"],
        Value::from(1)
    );
}

#[tokio::test]
async fn output_recovery_reloads_same_source_at_current_clock_without_advancing_queue() {
    use cumulus_core::playback::engine::EngineEvent;
    for (paused, reported, expected) in [
        (true, None, 42.5),
        (true, Some(42.1), 42.5),
        (true, Some(43.0), 43.0),
        (false, None, 42.5),
        (false, Some(43.0), 43.0),
    ] {
        let f = Fixture::new();
        f.command(replace(&[1])).await.unwrap();
        f.command(Cmd::Seek(42.5)).await.unwrap();
        f.command(Cmd::Pause(paused)).await.unwrap();
        let before = f.player.snapshot();
        let queue = f.player.queue(&f.token()).await.unwrap();
        {
            let mut native = f.device.state.lock().unwrap();
            native.view.position = reported;
            native.view.paused = true; // Failed output state is not user intent.
            native
                .view
                .events
                .push(EngineEvent::OutputReload { entry_id: 1 });
        }
        let after = until(&f.player, |s| {
            s.play_id != before.play_id
                && s.state
                    == if paused {
                        Status::Paused
                    } else {
                        Status::Playing
                    }
        })
        .await;
        assert_eq!(after.position, Some(expected));
        assert_eq!(after.selected, before.selected);
        assert_eq!(f.player.queue(&f.token()).await.unwrap(), queue);
        assert_eq!(f.api.count(1), 1, "reuse existing authorized source");
        assert_eq!(f.device.state.lock().unwrap().loads, 2);
        assert_eq!(f.device.state.lock().unwrap().view.position, Some(expected));
        f.command(Cmd::Stop).await.unwrap();
    }
}
