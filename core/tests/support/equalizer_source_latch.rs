use super::*;
use cumulus_core::{
    library::LocalLibrary,
    playback::{PlaybackQueue, controller::PlaybackController, source::SourceResolver},
};
use std::sync::Arc;

fn combined() -> EqualizerSettings {
    curve(4.0).with_tone(3, -2).unwrap().with_tone_enabled(true)
}
fn configured_fixture() -> Fixture {
    let f = Fixture::new();
    f.device.equalizer_supported.store(true, Ordering::SeqCst);
    f
}
fn level(f: &Fixture, id: u64, actual: serde_json::Value, bitrate: u32) {
    f.api
        .representations
        .lock()
        .unwrap()
        .insert(id, (actual, bitrate));
}
fn edit(expected: u64, settings: EqualizerSettings, preset: Option<EqualizerPreset>) -> Cmd {
    Cmd::Equalizer {
        expected_revision: expected,
        settings,
        preset,
    }
}
fn play(id: u64, requested: &str) -> Cmd {
    Cmd::Replace {
        resources: vec![json!({"id":id,"dt":120000,"level":requested})],
        index: 0,
        source: json!({"kind":"test"}),
    }
}
fn native(f: &Fixture) -> Option<EqualizerSettings> {
    *f.device.equalizer.lock().unwrap()
}
fn applications(f: &Fixture) -> Vec<EqualizerSettings> {
    f.device.equalizer_applications.lock().unwrap().clone()
}
async fn wait_for_eq_call(f: &Fixture) {
    let notified = tokio::time::timeout(
        Duration::from_secs(2),
        f.device.equalizer_started.notified(),
    )
    .await;
    if notified.is_err() {
        f.device.equalizer_hold.store(false, Ordering::SeqCst);
    }
    notified.expect("fixture did not reach the native EQ call");
}

#[tokio::test]
async fn actual_online_quality_alternates_the_latch_only_after_accepted_native_play() {
    let f = configured_fixture();
    let settings = combined();
    let saved = f.command(edit(0, settings, None)).await.unwrap();
    let before = applications(&f).len();
    let entries = [
        (Some("standard"), "vivid", 6999000, false),
        (Some("vivid"), "standard", 128000, true),
        (Some("dolby"), "standard", 128000, false),
        (Some("sky"), "standard", 128000, true),
        (Some("jymaster"), "standard", 4999000, false),
        (None, "standard", 6999000, false),
        (Some("jyeffect"), "standard", 128000, true),
        (Some("standard"), "standard", 128000, false),
    ];
    let active = settings.effective();
    let dry = EqualizerSettings::default();
    let mut previous = active;
    for (index, (actual, requested, br, bypass)) in entries.into_iter().enumerate() {
        let id = index as u64 + 1;
        level(&f, id, json!(actual), br);
        let view = f.command(play(id, requested)).await.unwrap();
        assert_eq!(view.state, PlaybackStatus::Playing);
        assert_eq!(view.actual_quality.as_deref(), actual);
        assert_eq!(view.equalizer.source_bypassed, bypass);
        assert_eq!(view.equalizer.preferences, saved.equalizer.preferences);
        // M/308 reads Ge after native Play succeeds, rather than consuming it
        // during authorization or before a failed native play acknowledgement.
        assert_eq!(
            f.device.equalizer_at_unpause.lock().unwrap()[index],
            Some(previous)
        );
        previous = if bypass { dry } else { active };
        assert_eq!(native(&f), Some(previous));
    }
    assert_eq!(
        applications(&f)[before..],
        [dry, active, dry, active, dry, active]
    );
    assert_eq!(f.device.state.lock().unwrap().loads, entries.len());
    assert_eq!(
        f.api.calls.lock().unwrap().as_slice(),
        &entries
            .iter()
            .enumerate()
            .map(|(i, (_, request, _, _))| (i as u64 + 1, (*request).to_owned()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        EqualizerPreferences::read(&f.store, "9").unwrap(),
        saved.equalizer.preferences
    );
}

#[tokio::test]
async fn save_slot_keeps_bypass_and_explicit_edit_or_retry_reapplies_without_consuming_ge() {
    let f = configured_fixture();
    let settings = combined();
    for (id, actual) in [(1, "sky"), (2, "dolby"), (3, "vivid")] {
        level(&f, id, json!(actual), 128000);
    }
    f.command(edit(0, settings, None)).await.unwrap();
    let dry = f.command(play(1, "standard")).await.unwrap();
    assert!(dry.equalizer.source_bypassed);
    let count = applications(&f).len();
    let saved = f
        .command(Cmd::SaveCustomEqualizer {
            expected_revision: 1,
        })
        .await
        .unwrap();
    assert!(saved.equalizer.source_bypassed);
    assert_eq!(applications(&f).len(), count);
    let activated = f
        .command(edit(1, settings, Some(EqualizerPreset::Manual)))
        .await
        .unwrap();
    assert_eq!(activated.play_id, dry.play_id);
    assert!(!activated.equalizer.source_bypassed);
    assert_eq!(activated.equalizer.preferences.revision, 2);
    assert_eq!(native(&f), Some(settings.effective()));
    assert_eq!(applications(&f).len(), count + 1);
    assert_eq!(f.api.count(1), 1);
    // A pause or native seek never re-enters the source latch.
    f.command(Cmd::Pause(true)).await.unwrap();
    f.command(Cmd::Seek(8.0)).await.unwrap();
    assert_eq!(applications(&f).len(), count + 1);
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    assert!(
        !f.command(play(2, "standard"))
            .await
            .unwrap()
            .equalizer
            .source_bypassed
    );
    let dry_again = f.command(play(3, "standard")).await.unwrap();
    assert!(dry_again.equalizer.source_bypassed);
    let retried = f.command(Cmd::RetryEqualizer).await.unwrap();
    assert_eq!(retried.play_id, dry_again.play_id);
    assert_eq!(
        retried.equalizer.preferences,
        dry_again.equalizer.preferences
    );
    assert!(!retried.equalizer.source_bypassed);
    assert_eq!(native(&f), Some(settings.effective()));
    assert_eq!(f.device.state.lock().unwrap().loads, 3);
}

#[tokio::test]
async fn a_master_off_source_keeps_ge_for_the_next_enabled_source() {
    let f = configured_fixture();
    let settings = combined();
    for id in 1..=4 {
        level(&f, id, json!("vivid"), 128000);
    }
    f.command(edit(0, settings, None)).await.unwrap();
    assert!(
        f.command(play(1, "standard"))
            .await
            .unwrap()
            .equalizer
            .source_bypassed
    );
    let mut off = serde_json::to_value(settings).unwrap();
    off["master_bypassed"] = json!(true);
    f.command(edit(1, serde_json::from_value(off).unwrap(), None))
        .await
        .unwrap();
    let count = applications(&f).len();
    assert!(
        !f.command(play(2, "standard"))
            .await
            .unwrap()
            .equalizer
            .source_bypassed
    );
    assert_eq!(applications(&f).len(), count);
    f.command(edit(2, settings, None)).await.unwrap();
    assert!(
        !f.command(play(3, "standard"))
            .await
            .unwrap()
            .equalizer
            .source_bypassed
    );
    assert!(
        f.command(play(4, "standard"))
            .await
            .unwrap()
            .equalizer
            .source_bypassed
    );
    assert_eq!(
        EqualizerPreferences::read(&f.store, "9").unwrap().settings,
        settings
    );
}

#[tokio::test]
async fn unsuccessful_native_and_cancelled_authorization_do_not_advance_ge() {
    let f = configured_fixture();
    let settings = combined();
    f.command(edit(0, settings, None)).await.unwrap();
    let count = applications(&f).len();
    for id in [1, 2, 3, 5] {
        level(&f, id, json!("dolby"), 128000);
    }
    f.device.load_failure_code.store(-13, Ordering::SeqCst);
    assert!(f.command(play(1, "standard")).await.is_err());
    f.device.load_failure_code.store(0, Ordering::SeqCst);
    f.device.fail_unpause.store(true, Ordering::SeqCst);
    assert!(f.command(play(2, "standard")).await.is_err());
    f.device.fail_unpause.store(false, Ordering::SeqCst);
    f.api.hold.store(3, Ordering::SeqCst);
    let stale = f.start(play(3, "standard"));
    tokio::time::timeout(Duration::from_secs(2), f.api.started.notified())
        .await
        .unwrap();
    let successor = f.command(play(4, "standard")).await.unwrap();
    f.api.hold.store(0, Ordering::SeqCst);
    f.api.release.notify_one();
    assert!(stale.await.unwrap().is_err());
    assert!(!successor.equalizer.source_bypassed);
    assert_eq!(applications(&f).len(), count);
    assert!(
        f.command(play(5, "standard"))
            .await
            .unwrap()
            .equalizer
            .source_bypassed
    );
}

#[tokio::test]
async fn a_peer_queue_change_during_native_eq_ack_cannot_publish_or_consume_ge() {
    let f = configured_fixture();
    let settings = combined();
    f.command(edit(0, settings, None)).await.unwrap();
    wait_for_eq_call(&f).await; // consume initial settings notification
    level(&f, 1, json!("vivid"), 128000);
    level(&f, 3, json!("sky"), 128000);
    f.device.equalizer_hold.store(true, Ordering::SeqCst);
    let old = f.start(play(1, "standard"));
    wait_for_eq_call(&f).await;
    let mut peer = PlaybackQueue::new(Some(f.store.clone()), "9").unwrap();
    let changed = peer.replace(vec![json!({"id":2,"dt":120000})], 0, json!({"kind":"test"}));
    f.device.equalizer_hold.store(false, Ordering::SeqCst);
    changed.unwrap();
    let error = old.await.unwrap().err().unwrap();
    assert!(matches!(root(&error), Error::QueueChanged));
    let rejected = f.player.snapshot();
    assert!(!rejected.equalizer.source_bypassed);
    assert!(f.device.state.lock().unwrap().view.entry_id.is_none());

    let accepted = f.command(play(2, "standard")).await.unwrap();
    assert!(!accepted.equalizer.source_bypassed);
    assert_eq!(native(&f), Some(settings.effective()));
    // Had the stale native acknowledgement committed the old latch, this
    // special source could have taken the restore branch instead.
    assert!(
        f.command(play(3, "standard"))
            .await
            .unwrap()
            .equalizer
            .source_bypassed
    );
}

#[tokio::test]
async fn account_change_during_restore_clears_old_native_state_and_ge() {
    let f = configured_fixture();
    let settings = combined();
    f.command(edit(0, settings, None)).await.unwrap();
    for id in 1..=3 {
        level(&f, id, json!("vivid"), 128000);
    }
    assert!(
        f.command(play(1, "standard"))
            .await
            .unwrap()
            .equalizer
            .source_bypassed
    );
    wait_for_eq_call(&f).await;
    f.device.equalizer_hold.store(true, Ordering::SeqCst);
    let stale = f.start(play(2, "standard"));
    wait_for_eq_call(&f).await;
    let switched = f
        .session
        .adopt(f.api.clone(), json!({"userId":10}), &f.token());
    f.device.equalizer_hold.store(false, Ordering::SeqCst);
    switched.unwrap();
    assert!(stale.await.unwrap().is_err());
    let rebound = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(rebound.account.uid.as_deref(), Some("10"));
    assert!(!rebound.equalizer.source_bypassed);
    assert_eq!(rebound.equalizer.preferences.revision, 0);
    assert_eq!(native(&f), Some(EqualizerSettings::default()));
    assert_eq!(
        EqualizerPreferences::read(&f.store, "9").unwrap().settings,
        settings
    );
    f.command(edit(0, curve(-5.0), None)).await.unwrap();
    assert!(
        f.command(play(3, "standard"))
            .await
            .unwrap()
            .equalizer
            .source_bypassed
    );
}

#[tokio::test]
async fn known_local_path_does_not_start_a_latch_from_a_requested_quality_label() {
    let f = configured_fixture();
    let settings = combined();
    let library = Arc::new(LocalLibrary::new(f.store.clone()).unwrap());
    let resolver = Arc::new(SourceResolver::new(f.session.clone(), Some(library)));
    let path = f.temp.path().join("synthetic.flac");
    std::fs::write(&path, include_bytes!("../fixtures/tag_audio/seed.flac")).unwrap();
    let device = Arc::new(Device::default());
    device.equalizer_supported.store(true, Ordering::SeqCst);
    let player = PlaybackController::start(
        f.session.clone(),
        f.store.clone(),
        resolver,
        audio_engine(device.clone()),
    )
    .unwrap();
    let local = || Cmd::Replace {
        resources: vec![json!({"path":path,"level":"vivid"})],
        index: 0,
        source: json!({"kind":"local"}),
    };
    let token = f.token();
    player
        .command(&token, edit(0, settings, None))
        .await
        .unwrap();
    let first = player.command(&token, local()).await.unwrap();
    assert!(!first.equalizer.source_bypassed);
    assert!(f.api.calls.lock().unwrap().is_empty());
    level(&f, 1, json!("dolby"), 128000);
    assert!(
        player
            .command(&token, play(1, "standard"))
            .await
            .unwrap()
            .equalizer
            .source_bypassed
    );
    let restored = player.command(&token, local()).await.unwrap();
    assert!(!restored.equalizer.source_bypassed);
    assert_eq!(
        *device.equalizer.lock().unwrap(),
        Some(settings.effective())
    );
    assert_eq!(f.api.count(1), 1);
}

#[tokio::test]
async fn newer_durable_curve_wins_a_held_source_apply_without_a_stale_latch_commit() {
    let f = configured_fixture();
    let old_curve = combined();
    let new_curve = curve(-7.0);
    f.command(edit(0, old_curve, None)).await.unwrap();
    wait_for_eq_call(&f).await;
    for id in [1, 2] {
        level(&f, id, json!("vivid"), 128000);
    }
    f.device.equalizer_hold.store(true, Ordering::SeqCst);
    let applying = f.start(play(1, "standard"));
    wait_for_eq_call(&f).await;
    let saved = EqualizerPreferences::replace(&f.store, "9", 1, new_curve, None);
    f.device.equalizer_hold.store(false, Ordering::SeqCst);
    let saved = saved.unwrap();
    let accepted = applying.await.unwrap().unwrap();
    assert_eq!(accepted.equalizer.preferences, saved);
    assert!(!accepted.equalizer.source_bypassed);
    let synced = f.command(Cmd::Snapshot).await.unwrap();
    assert_eq!(synced.play_id, accepted.play_id);
    assert_eq!(native(&f), Some(new_curve));
    assert!(
        f.command(play(2, "standard"))
            .await
            .unwrap()
            .equalizer
            .source_bypassed
    );
    assert_eq!(EqualizerPreferences::read(&f.store, "9").unwrap(), saved);
}
