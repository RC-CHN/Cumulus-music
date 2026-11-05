use super::*;
use cumulus_core::{
    playback::{TimelineMode, controller::PlaybackStatus},
    podcast::AudiobookSkip,
};
use std::time::Duration;

#[tokio::test]
async fn audiobook_tail_stops_native_decoder_and_records_complete_episode_without_native_eof() {
    let (root, api, _session, client, device) = fixture_with_device();
    api.count.store(2, Ordering::SeqCst);
    api.audiobook.store(true, Ordering::SeqCst);
    api.resume.store(true, Ordering::SeqCst);
    api.progress_revision.store(1, Ordering::SeqCst);
    device.state.lock().unwrap().duration = 240.0;
    let store = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    AudiobookSkip::Custom { start: 32, end: 20 }
        .save(&store.store(), "9", "7")
        .unwrap();
    let first = played_voice_from(&client, "1", Some("1")).await;
    assert_eq!(
        first.position,
        Some(32.0),
        "intro overrides the 15s breakpoint"
    );
    let before = device.state.lock().unwrap().stops;
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 222.0 })
        .await
        .unwrap();
    assert!(api.progress.lock().unwrap().is_empty());
    client
        .player_action("1", PlayerAction::Seek { seconds: 219.8 })
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Pause { paused: false })
        .await
        .unwrap();
    device.state.lock().unwrap().view.position = Some(220.0);
    assert_eq!(
        wait_for_progress(&api, 1).await,
        [json!({"programId":1,"listenLocation":240000.0,"isListened":true})]
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let playing = client
                .subscribe_playback("1")
                .await
                .unwrap()
                .current()
                .unwrap();
            if playing.state == PlaybackStatus::Playing
                && playing.track.as_ref().and_then(|t| t.voice_id.as_deref()) == Some("2")
            {
                assert_eq!(playing.position, Some(32.0));
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(device.state.lock().unwrap().stops > before);
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|(p, _)| p.ends_with("/skip/info/get"))
    );
}

#[tokio::test]
async fn smart_is_bound_to_the_authorized_full_episode_not_its_trial_window() {
    for trial in [false, true] {
        let (root, api, _session, client, device) = fixture_with_device();
        api.count.store(1, Ordering::SeqCst);
        api.audiobook.store(true, Ordering::SeqCst);
        api.trial.store(trial, Ordering::SeqCst);
        device.state.lock().unwrap().duration = 240.0;
        let store = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        AudiobookSkip::Smart.save(&store.store(), "9", "7").unwrap();
        let state = played_voice_from(&client, "1", Some("1")).await;
        assert_eq!(
            state.timeline.unwrap().mode,
            if trial {
                TimelineMode::Trial
            } else {
                TimelineMode::Full
            }
        );
        assert_eq!(state.position, Some(if trial { 0.0 } else { 26.0 }));
        let read = api
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.ends_with("/skip/info/get"))
            .count();
        assert_eq!(read, usize::from(!trial));
    }
}
