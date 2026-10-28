use super::*;
use cumulus_core::{
    client::native_media::{NativeMediaAction as Action, NativeMediaStatus as Status},
    playback::Step,
};

async fn setup() -> Fixture {
    let f = Fixture::new();
    f.client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["7".into(), "8".into(), "9".into()],
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    f
}

#[tokio::test]
async fn system_steps_preserve_pause_without_any_intermediate_unpause() {
    let f = setup().await;
    f.client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    f.audio.state.lock().unwrap().pauses.clear();
    for (step, song) in [(Step::Next, "8"), (Step::Previous, "7")] {
        let target = f.client.native_media_snapshot().unwrap().target.unwrap();
        f.client.native_media_step(&target, step).await.unwrap();
        let view = f
            .client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap();
        assert_eq!(view.track.unwrap().id, song);
        assert_eq!(
            f.client.native_media_details().unwrap().state,
            Status::Paused
        );
        assert!(matches!(
            f.client.native_media_step(&target, step).await,
            Err(Error::StaleResponse)
        ));
    }
    assert_eq!(f.audio.state.lock().unwrap().pauses, [true, true]);
    // Explicit manual/tray next still starts playback, matching the recovered UI.
    let target = f.client.native_media_snapshot().unwrap().target.unwrap();
    f.client
        .native_media_action(&target, Action::Next)
        .await
        .unwrap();
    assert_eq!(
        f.client.native_media_details().unwrap().state,
        Status::Playing
    );
}

#[tokio::test]
async fn stopped_steps_change_the_selection_without_loading_or_starting_audio() {
    let f = setup().await;
    f.client
        .player_action("1", PlayerAction::Stop)
        .await
        .unwrap();
    let loads = f.audio.state.lock().unwrap().loads;
    for step in [Step::Next, Step::Previous, Step::Next] {
        let target = f.client.native_media_snapshot().unwrap().target.unwrap();
        f.client.native_media_step(&target, step).await.unwrap();
        let view = f.client.native_media_details().unwrap();
        assert_eq!(view.state, Status::Stopped);
        assert!(!view.can_seek);
    }
    // Allow actor polling: it must not resurrect the previous decoder selection.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(f.audio.state.lock().unwrap().loads, loads);
    let view = f
        .client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_eq!(view.track.unwrap().id, "8");
    let target = f.client.native_media_snapshot().unwrap().target.unwrap();
    f.client
        .native_media_action(&target, Action::Play)
        .await
        .unwrap();
    assert_eq!(
        f.client.native_media_details().unwrap().state,
        Status::Playing
    );
    assert_eq!(f.audio.state.lock().unwrap().loads, loads + 1);
}

#[tokio::test]
async fn system_step_cannot_start_previous_audio_while_a_video_owns_media() {
    let f = setup().await;
    let audio = f.client.native_media_snapshot().unwrap().target.unwrap();
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    let video = f.client.native_media_snapshot().unwrap().target.unwrap();
    assert!(matches!(
        f.client.native_media_step(&audio, Step::Next).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        f.client.native_media_step(&video, Step::Next).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(f.audio.state.lock().unwrap().view.paused);
    assert!(!f.video.last().lock().unwrap().paused);
}

#[tokio::test]
async fn paused_loading_keeps_the_native_play_label_and_first_presentation_paused() {
    let f = setup().await;
    f.client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    // Consume the first load notification before blocking the next load.
    f.audio.started.notified().await;
    f.audio.state.lock().unwrap().pauses.clear();
    f.audio.hold_load.store(true, Ordering::Release);
    let client = f.client.clone();
    let target = client.native_media_snapshot().unwrap().target.unwrap();
    let pending = tokio::spawn(async move { client.native_media_step(&target, Step::Next).await });
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        f.audio.started.notified(),
    )
    .await
    .unwrap();
    let view = f.client.native_media_details().unwrap();
    assert_eq!(view.state, Status::Loading);
    assert!(!view.transport.playing);
    assert!(!view.can_seek);
    f.audio.hold_load.store(false, Ordering::Release);
    pending.await.unwrap().unwrap();
    assert_eq!(
        f.client.native_media_details().unwrap().state,
        Status::Paused
    );
    assert_eq!(f.audio.state.lock().unwrap().pauses, [true]);
}
