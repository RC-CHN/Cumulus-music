use super::*;
use cumulus_core::playback::{PlaybackMode, controller::PlaybackStatus};
use std::time::Duration;

#[tokio::test]
async fn playback_default_is_local_and_checks_current_membership_and_account_revision() {
    let dir = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    let client = MusicClient::with_session(session.clone(), registry).with_audio_factory(|| {
        panic!("a settings read or write must not initialize the audio device")
    });

    let guest = client
        .playback_quality_preferences("0", true)
        .await
        .unwrap();
    let rows = serde_json::to_value(&guest).unwrap();
    assert_eq!(
        rows["choices"],
        json!([
            {"quality":"sky","access":"unknown"},
            {"quality":"jymaster","access":"unknown"},
            {"quality":"jyeffect","access":"unknown"},
            {"quality":"hires","access":"unknown"},
            {"quality":"lossless","access":"unknown"},
            {"quality":"exhigh","access":"allowed"},
            {"quality":"standard","access":"allowed"}
        ])
    );
    let guest_revision = guest.revision.unwrap();
    assert!(matches!(
        client.save_playback_quality_preference("0", &guest_revision, Quality::Lossless),
        Err(Error::InvalidInput(
            "playback quality preference is unavailable"
        ))
    ));
    let guest = client
        .save_playback_quality_preference("0", &guest_revision, Quality::ExHigh)
        .unwrap();
    assert_eq!(guest.quality, Quality::ExHigh);
    assert!(api.calls.lock().unwrap().is_empty());

    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client.save_playback_quality_preference("0", &guest.revision.unwrap(), Quality::Standard),
        Err(Error::StaleResponse)
    ));
    let unknown = client
        .playback_quality_preferences("1", false)
        .await
        .unwrap();
    assert_eq!(unknown.quality, Quality::Standard);
    assert!(api.calls.lock().unwrap().is_empty());
    let unknown_revision = unknown.revision.unwrap();
    assert!(matches!(
        client.save_playback_quality_preference("1", &unknown_revision, Quality::HiRes),
        Err(Error::InvalidInput(
            "playback quality preference is unavailable"
        ))
    ));
    let current = client
        .playback_quality_preferences("1", true)
        .await
        .unwrap();
    let current_revision = current.revision.unwrap();
    assert!(matches!(
        client.save_playback_quality_preference("1", &unknown_revision, Quality::ExHigh),
        Err(Error::State("listening quality settings changed"))
    ));
    assert!(matches!(
        client.save_playback_quality_preference("1", &current_revision, Quality::JyMaster),
        Err(Error::InvalidInput(
            "playback quality preference is unavailable"
        ))
    ));
    let saved = client
        .save_playback_quality_preference("1", &current_revision, Quality::HiRes)
        .unwrap();
    assert_eq!(saved.quality, Quality::HiRes);
    let saved_revision = saved.revision.unwrap();
    assert_eq!(
        client
            .save_playback_quality_preference("1", &saved_revision, Quality::HiRes)
            .unwrap()
            .revision
            .as_deref(),
        Some(saved_revision.as_str()),
        "reselecting the same allowed quality does not write a new revision"
    );
    assert_eq!(
        api.calls.lock().unwrap().as_slice(),
        ["/api/music-vip-membership/client/vip/info"]
    );
    assert_eq!(api.quality_writes.load(Ordering::SeqCst), 0);
    assert_eq!(
        cumulus_core::playback::quality_preferences::QualityPreferences::read(
            &store,
            "local-guest"
        )
        .unwrap()
        .quality,
        Quality::ExHigh
    );

    let malformed = json!({"broken":true});
    store
        .put("9", "playback-quality-v1", "current", &malformed)
        .unwrap();
    let damaged = client
        .playback_quality_preferences("1", false)
        .await
        .unwrap();
    assert!(damaged.damaged && damaged.revision.is_none());
    assert!(matches!(
        client.save_playback_quality_preference("1", &saved_revision, Quality::Standard),
        Err(Error::State("listening quality preference damaged"))
    ));
    assert_eq!(
        store.get("9", "playback-quality-v1", "current").unwrap(),
        Some(malformed)
    );
}

#[tokio::test]
async fn playback_default_changes_next_authorization_without_touching_the_paused_source() {
    let device = Arc::new(support::Device::default());
    let (_dir, api, _session, client, _) = fixture_with_device(device.clone());
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Sequential,
            },
        )
        .await
        .unwrap();
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["1".into(), "2".into()],
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let paused = client
        .player_action("1", PlayerAction::Seek { seconds: 10.0 })
        .await
        .unwrap();
    let loads = device.state.lock().unwrap().loads;
    let preferences = client
        .playback_quality_preferences("1", false)
        .await
        .unwrap();
    client
        .save_playback_quality_preference("1", &preferences.revision.unwrap(), Quality::ExHigh)
        .unwrap();
    let same = client.player_snapshot("1").await.unwrap();
    assert_eq!(same.play_id, paused.play_id);
    assert_eq!(same.entry_id, paused.entry_id);
    assert_eq!(same.queue_revision, paused.queue_revision);
    assert_eq!(same.state, PlaybackStatus::Paused);
    assert_eq!(same.position, Some(10.0));
    assert_eq!(same.requested_quality.as_deref(), Some("standard"));
    assert_eq!(same.quality_preferences.quality, Quality::ExHigh);
    assert_eq!(device.state.lock().unwrap().loads, loads);
    let next = client.player_action("1", PlayerAction::Next).await.unwrap();
    assert_eq!(next.track.unwrap().id, "2");
    assert_eq!(next.requested_quality.as_deref(), Some("exhigh"));
    assert_eq!(device.state.lock().unwrap().loads, loads + 1);
    let calls = api.song_authorizations.lock().unwrap().clone();
    assert_eq!(
        calls.iter().filter(|(ids, _)| *ids == json!(["1"])).count(),
        1
    );
    assert!(calls.contains(&(json!(["2"]), json!("exhigh"))));
    assert_eq!(api.quality_writes.load(Ordering::SeqCst), 0);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn playback_default_cannot_overwrite_a_pending_or_bound_trial() {
    let (_dir, api, _session, client, _) = fixture();
    let playing = client.play_selection("1", songs("1")).await.unwrap();
    let play_id = playing.play_id.unwrap();
    let details = client.player_quality_details("1", &play_id).await.unwrap();
    let revision = client
        .playback_quality_preferences("1", false)
        .await
        .unwrap()
        .revision
        .unwrap();
    api.quality_start_mode.store(1, Ordering::SeqCst);
    let pending = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .player_quality_trial(
                    "1",
                    QualityTrialCommand::Start {
                        play_id,
                        details_revision: details.revision,
                        level: Quality::JyMaster,
                    },
                )
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(3), api.quality_started.notified())
        .await
        .unwrap();
    let locked = client
        .playback_quality_preferences("1", false)
        .await
        .unwrap();
    assert!(locked.revision.is_none() && !locked.damaged);
    assert!(matches!(
        client.save_playback_quality_preference("1", &revision, Quality::ExHigh),
        Err(Error::MutationBusy)
    ));
    api.quality_release.notify_one();
    let finished = pending.await.unwrap().unwrap();
    assert!(finished.playback.is_some());
    let locked = client
        .playback_quality_preferences("1", false)
        .await
        .unwrap();
    assert!(locked.revision.is_none() && !locked.damaged);
    assert_eq!(locked.quality, Quality::JyMaster);
    assert!(matches!(
        client.save_playback_quality_preference("1", &revision, Quality::ExHigh),
        Err(Error::State("listening quality trial blocks settings"))
    ));
    assert_eq!(api.quality_writes.load(Ordering::SeqCst), 1);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}
