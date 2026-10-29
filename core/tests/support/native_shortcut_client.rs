use super::*;
use cumulus_core::client::native_media::ShortcutSelection;
use cumulus_core::preferences::shortcuts::{Action, Hotkey};
fn key(action: Action) -> Hotkey {
    Hotkey::Configured(action)
}

#[tokio::test]
async fn local_keys_require_the_account_and_selection_observed_before_ipc() {
    let f = Fixture::new();
    f.client.play_selection("1", songs()).await.unwrap();
    let audio = f
        .client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    let mut observed = ShortcutSelection {
        audio_play_id: audio.play_id,
        audio_entry_id: audio.entry_id,
        video_play_id: None,
    };
    let f5 = f
        .client
        .capture_local_shortcut("1", &observed, Hotkey::F5)
        .unwrap()
        .unwrap();
    f.client.execute_native_shortcut(&f5).await.unwrap();
    assert!(f.audio.state.lock().unwrap().view.paused);
    let next = f
        .client
        .capture_native_shortcut(key(Action::Next))
        .unwrap()
        .unwrap();
    f.client.execute_native_shortcut(&next).await.unwrap();
    assert!(matches!(
        f.client
            .capture_local_shortcut("1", &observed, Hotkey::Space),
        Err(Error::StaleResponse)
    ));
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    assert!(matches!(
        f.client.capture_local_shortcut("1", &observed, Hotkey::F5),
        Err(Error::StaleResponse)
    ));
    observed.video_play_id = Some(f.client.video_snapshot("1").await.unwrap().play_id);
    let video = f
        .client
        .capture_local_shortcut("1", &observed, Hotkey::F5)
        .unwrap()
        .unwrap();
    f.client.execute_native_shortcut(&video).await.unwrap();
    assert!(f.video.last().lock().unwrap().paused);
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.client.capture_local_shortcut("1", &observed, Hotkey::F5),
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn shortcut_capture_is_lazy_and_old_targets_cannot_control_a_new_selection_or_account() {
    let f = Fixture::new();
    assert!(
        f.client
            .capture_native_shortcut(key(Action::PlayPause))
            .unwrap()
            .is_none()
    );
    assert_eq!(f.audio.state.lock().unwrap().loads, 0);
    assert!(f.video.api.calls.lock().unwrap().is_empty());
    f.client.play_selection("1", songs()).await.unwrap();
    let pause = f
        .client
        .capture_native_shortcut(key(Action::PlayPause))
        .unwrap()
        .unwrap();
    f.client.execute_native_shortcut(&pause).await.unwrap();
    assert!(f.audio.state.lock().unwrap().view.paused);
    assert!(matches!(
        f.client.execute_native_shortcut(&pause).await,
        Err(Error::StaleResponse)
    ));
    let next = f
        .client
        .capture_native_shortcut(key(Action::Next))
        .unwrap()
        .unwrap();
    f.client.execute_native_shortcut(&next).await.unwrap();
    assert!(
        !f.audio.state.lock().unwrap().view.paused,
        "official manual shortcut next starts playback"
    );
    let old = f
        .client
        .capture_native_shortcut(Hotkey::MediaStop)
        .unwrap()
        .unwrap();
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.client.execute_native_shortcut(&old).await,
        Err(Error::StaleResponse)
    ));
}
#[tokio::test]
async fn shortcut_volume_uses_official_five_percent_step_and_preserves_video_ownership() {
    let f = Fixture::new();
    f.client.play_selection("1", songs()).await.unwrap();
    f.client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    f.client
        .player_action("1", PlayerAction::Volume { value: 97.0 })
        .await
        .unwrap();
    for (action, expected) in [(Action::VolumeUp, 100.0), (Action::VolumeDown, 95.0)] {
        let invocation = f
            .client
            .capture_native_shortcut(key(action))
            .unwrap()
            .unwrap();
        f.client.execute_native_shortcut(&invocation).await.unwrap();
        assert_eq!(f.client.native_media_details().unwrap().volume, expected);
        assert!(f.audio.state.lock().unwrap().view.paused);
    }
    let old = f
        .client
        .capture_native_shortcut(key(Action::VolumeUp))
        .unwrap()
        .unwrap();
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    assert!(matches!(
        f.client.execute_native_shortcut(&old).await,
        Err(Error::StaleResponse)
    ));
    f.client
        .video_action("1", VideoAction::Volume { value: 3.0 })
        .await
        .unwrap();
    let down = f
        .client
        .capture_native_shortcut(key(Action::VolumeDown))
        .unwrap()
        .unwrap();
    f.client.execute_native_shortcut(&down).await.unwrap();
    assert_eq!(f.client.native_media_details().unwrap().volume, 0.0);
    for hotkey in [key(Action::Previous), key(Action::Next), Hotkey::MediaStop] {
        assert!(f.client.capture_native_shortcut(hotkey).unwrap().is_none());
    }
    let toggle = f
        .client
        .capture_native_shortcut(Hotkey::MediaPlayPause)
        .unwrap()
        .unwrap();
    f.client.execute_native_shortcut(&toggle).await.unwrap();
    assert!(f.video.last().lock().unwrap().paused);
    assert!(f.audio.state.lock().unwrap().view.paused);
}
#[tokio::test]
async fn like_shortcut_never_guesses_unknown_state_or_replays_a_lost_reply() {
    let f = Fixture::new();
    f.client.play_selection("1", songs()).await.unwrap();
    assert!(f.client.capture_native_shortcut(key(Action::Like)).is_err());
    assert!(f.api.like_writes.lock().unwrap().is_empty());
    f.client.liked_ids().await.unwrap();
    let invocation = f
        .client
        .capture_native_shortcut(key(Action::Like))
        .unwrap()
        .unwrap();
    let selection = invocation.like_selection().unwrap();
    f.api.lose_like.store(true, Ordering::Release);
    assert!(f.client.execute_native_shortcut(&invocation).await.is_err());
    assert!(matches!(
        f.client.capture_native_shortcut(key(Action::Like)),
        Err(Error::MutationUncertain)
    ));
    assert!(f.client.execute_native_shortcut(&invocation).await.is_err());
    let result = f.client.native_like_refresh(selection).await.unwrap();
    assert_eq!(result.observed, Some(true));
    assert!(!result.pending);
    assert_eq!(f.api.like_writes.lock().unwrap().len(), 1);
}
