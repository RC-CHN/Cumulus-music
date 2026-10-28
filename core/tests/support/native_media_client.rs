use super::*;
use cumulus_core::client::native_media::NativeMediaAction as Action;
#[tokio::test]
async fn native_projection_is_lazy_and_audio_toggle_stop_use_existing_player() {
    let f = Fixture::new();
    assert!(f.client.native_media_snapshot().unwrap().target.is_none());
    assert_eq!(f.audio.state.lock().unwrap().loads, 0);
    assert!(f.video.devices.lock().unwrap().is_empty());
    assert!(f.video.api.calls.lock().unwrap().is_empty());
    f.client.play_selection("1", songs()).await.unwrap();
    let first = f.client.native_media_snapshot().unwrap();
    assert!(first.playing && first.can_next && first.can_previous);
    f.client
        .native_media_action(first.target.as_ref().unwrap(), Action::Toggle)
        .await
        .unwrap();
    assert!(f.audio.state.lock().unwrap().view.paused);
    assert!(matches!(
        f.client
            .native_media_action(first.target.as_ref().unwrap(), Action::Stop)
            .await,
        Err(Error::StaleResponse)
    ));
    let paused = f.client.native_media_snapshot().unwrap();
    assert!(!paused.playing);
    f.client
        .native_media_action(paused.target.as_ref().unwrap(), Action::Toggle)
        .await
        .unwrap();
    assert!(!f.audio.state.lock().unwrap().view.paused);
    let current = f.client.native_media_snapshot().unwrap();
    f.client
        .native_media_action(current.target.as_ref().unwrap(), Action::Stop)
        .await
        .unwrap();
    assert!(!f.client.native_media_snapshot().unwrap().playing);
}
#[tokio::test]
async fn native_mv_controls_preserve_music_pause_and_reject_stale_focus_and_account() {
    let f = Fixture::new();
    f.client.play_selection("1", songs()).await.unwrap();
    let audio = f.client.native_media_snapshot().unwrap().target.unwrap();
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    for action in [Action::Play, Action::Pause, Action::Stop] {
        assert!(matches!(
            f.client.native_media_action(&audio, action).await,
            Err(Error::StaleResponse)
        ));
    }
    assert!(!f.video.last().lock().unwrap().paused);
    let video = f.client.native_media_snapshot().unwrap();
    assert!(!video.can_next && !video.can_previous);
    assert!(matches!(
        f.client
            .native_media_action(video.target.as_ref().unwrap(), Action::Next)
            .await,
        Err(Error::InvalidInput(_))
    ));
    f.client
        .native_media_action(video.target.as_ref().unwrap(), Action::Pause)
        .await
        .unwrap();
    assert!(f.video.last().lock().unwrap().paused);
    let paused = f.client.native_media_snapshot().unwrap();
    f.client
        .native_media_action(paused.target.as_ref().unwrap(), Action::Play)
        .await
        .unwrap();
    assert!(!f.video.last().lock().unwrap().paused);
    assert!(f.audio.state.lock().unwrap().view.paused);
    let video = f.client.native_media_snapshot().unwrap().target.unwrap();
    f.client
        .player_action("1", PlayerAction::Play)
        .await
        .unwrap();
    assert!(matches!(
        f.client.native_media_action(&video, Action::Pause).await,
        Err(Error::StaleResponse)
    ));
    assert!(!f.audio.state.lock().unwrap().view.paused);
    let audio = f.client.native_media_snapshot().unwrap().target.unwrap();
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.client.native_media_action(&audio, Action::Next).await,
        Err(Error::StaleResponse)
    ));
    assert!(f.client.native_media_snapshot().unwrap().target.is_none());
}
#[tokio::test]
async fn native_targets_survive_clock_ticks_but_not_seek_or_new_selection() {
    let f = Fixture::new();
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    let target = f.client.native_media_snapshot().unwrap().target.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    f.client
        .native_media_action(&target, Action::Pause)
        .await
        .unwrap();
    let target = f.client.native_media_snapshot().unwrap().target.unwrap();
    f.client
        .video_action("1", VideoAction::Seek { seconds: 20.0 })
        .await
        .unwrap();
    assert!(matches!(
        f.client.native_media_action(&target, Action::Play).await,
        Err(Error::StaleResponse)
    ));
    f.client.play_selection("1", songs()).await.unwrap();
    let target = f.client.native_media_snapshot().unwrap().target.unwrap();
    f.client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["8".into()],
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        f.client.native_media_action(&target, Action::Stop).await,
        Err(Error::StaleResponse)
    ));
    assert!(!f.audio.state.lock().unwrap().view.paused);
}
