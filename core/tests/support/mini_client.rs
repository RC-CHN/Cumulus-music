use super::*;
use cumulus_core::client::mini::MiniAction as Action;

#[tokio::test]
async fn mini_shortcut_selection_is_lazy_and_cannot_control_a_new_owner() {
    use cumulus_core::{client::native_media::ShortcutSelection, preferences::shortcuts::Hotkey};
    let f = Fixture::new();
    assert_eq!(
        f.client.mini_snapshot().unwrap().view.selection,
        ShortcutSelection::default()
    );
    assert_eq!(f.audio.state.lock().unwrap().loads, 0);
    assert!(f.video.devices.lock().unwrap().is_empty());
    assert!(f.video.api.calls.lock().unwrap().is_empty());
    f.client.play_selection("1", songs()).await.unwrap();
    let observed = f.client.mini_snapshot().unwrap().view.selection;
    let invocation = f
        .client
        .capture_local_shortcut("1", &observed, Hotkey::Space)
        .unwrap()
        .unwrap();
    f.client.execute_native_shortcut(&invocation).await.unwrap();
    assert!(f.audio.state.lock().unwrap().view.paused);
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    assert!(matches!(
        f.client
            .capture_local_shortcut("1", &observed, Hotkey::Space),
        Err(Error::StaleResponse)
    ));
    let video = f.client.mini_snapshot().unwrap().view.selection;
    assert!(
        video.audio_play_id.is_none()
            && video.audio_entry_id.is_none()
            && video.video_play_id.is_some()
    );
    let invocation = f
        .client
        .capture_local_shortcut("1", &video, Hotkey::Space)
        .unwrap()
        .unwrap();
    f.client.execute_native_shortcut(&invocation).await.unwrap();
    assert!(f.video.last().lock().unwrap().paused);
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.client.capture_local_shortcut("1", &video, Hotkey::Space),
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn mini_reads_are_lazy_public_and_share_the_existing_audio_actor() {
    let f = Fixture::new();
    let empty = f.client.mini_snapshot().unwrap();
    assert!(empty.view.item.is_none());
    assert!(f.client.mini_action(&empty, Action::Toggle).await.is_err());
    assert!(f.client.mini_queue(&empty, 0, 10).await.is_err());
    assert_eq!(f.audio.state.lock().unwrap().loads, 0);
    assert!(f.video.api.calls.lock().unwrap().is_empty());
    assert!(f.video.devices.lock().unwrap().is_empty());
    f.client.play_selection("1", songs()).await.unwrap();
    let first = f.client.mini_snapshot().unwrap();
    let loads = f.audio.state.lock().unwrap().loads;
    let public = serde_json::to_string(&first.view).unwrap();
    assert!(
        !public.contains("SECRET") && !public.contains("signature") && !public.contains("target")
    );
    assert_eq!(first.view.item.as_ref().unwrap().title, "Song");
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    assert!(first.same_controls(&f.client.mini_snapshot().unwrap()));
    f.client.mini_action(&first, Action::Toggle).await.unwrap();
    assert!(f.audio.state.lock().unwrap().view.paused);
    assert_eq!(f.audio.state.lock().unwrap().loads, loads);
    assert!(!first.same_controls(&f.client.mini_snapshot().unwrap()));
    assert!(matches!(
        f.client.mini_action(&first, Action::Toggle).await,
        Err(Error::StaleResponse)
    ));
    let paused = f.client.mini_snapshot().unwrap();
    f.client.mini_action(&paused, Action::Toggle).await.unwrap();
    assert!(!f.audio.state.lock().unwrap().view.paused);
    assert_eq!(f.audio.state.lock().unwrap().loads, loads);
}

#[tokio::test]
async fn mini_queue_is_paged_and_rejects_old_queue_selection_and_clear() {
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
    let first = f.client.mini_snapshot().unwrap();
    let page = f.client.mini_queue(&first, 1, 1).await.unwrap();
    assert_eq!((page.offset, page.total, page.tracks.len()), (1, 3, 1));
    assert_eq!(page.tracks[0].track.id, "8");
    assert!(page.editable);
    let removed = page.tracks[0].entry_id.clone();
    assert!(f.client.mini_queue(&first, 0, 101).await.is_err());
    assert!(f.client.mini_queue(&first, 0, 0).await.is_err());
    f.client
        .mini_action(
            &first,
            Action::Remove {
                entry_id: removed.clone(),
            },
        )
        .await
        .unwrap();
    for action in [Action::Select { entry_id: removed }, Action::Clear] {
        assert!(matches!(
            f.client.mini_action(&first, action).await,
            Err(Error::StaleResponse)
        ));
    }
    assert!(matches!(
        f.client.mini_queue(&first, 0, 10).await,
        Err(Error::StaleResponse)
    ));
    let current = f.client.mini_snapshot().unwrap();
    let queue = f.client.mini_queue(&current, 0, 10).await.unwrap();
    assert_eq!(queue.total, 2);
    f.client
        .mini_action(
            &current,
            Action::Select {
                entry_id: queue.tracks[1].entry_id.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.client.player_queue("1").await.unwrap().current,
        Some(queue.tracks[1].entry_id.clone())
    );
    let selected = f.client.mini_snapshot().unwrap();
    f.client
        .mini_action(&selected, Action::Clear)
        .await
        .unwrap();
    assert!(f.client.mini_snapshot().unwrap().view.item.is_none());
    assert!(f.client.player_queue("1").await.unwrap().tracks.is_empty());
}

#[tokio::test]
async fn mini_mv_ownership_and_account_change_invalidate_old_commands() {
    let f = Fixture::new();
    f.client.play_selection("1", songs()).await.unwrap();
    let audio = f.client.mini_snapshot().unwrap();
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    assert!(matches!(
        f.client.mini_action(&audio, Action::Toggle).await,
        Err(Error::StaleResponse)
    ));
    let video = f.client.mini_snapshot().unwrap();
    assert!(video.view.like.is_none() && video.view.mode.is_none());
    assert!(f.client.mini_queue(&video, 0, 10).await.is_err());
    assert!(f.client.mini_action(&video, Action::Next).await.is_err());
    f.client.mini_action(&video, Action::Toggle).await.unwrap();
    assert!(f.video.last().lock().unwrap().paused);
    assert!(f.audio.state.lock().unwrap().view.paused);
    let paused = f.client.mini_snapshot().unwrap();
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.client.mini_action(&paused, Action::Toggle).await,
        Err(Error::StaleResponse)
    ));
    assert!(f.client.mini_snapshot().unwrap().view.item.is_none());
}

#[tokio::test]
async fn mini_like_unknown_and_lost_reply_require_explicit_read_not_replay() {
    let f = Fixture::new();
    f.client.play_selection("1", songs()).await.unwrap();
    let unknown = f.client.mini_snapshot().unwrap();
    assert!(f.client.mini_action(&unknown, Action::Like).await.is_err());
    assert!(f.api.like_writes.lock().unwrap().is_empty());
    f.client
        .mini_action(&unknown, Action::RefreshLike)
        .await
        .unwrap();
    let known = f.client.mini_snapshot().unwrap();
    assert_eq!(known.view.like.as_ref().unwrap().observed, Some(false));
    assert!(!known.same_controls(&unknown));
    f.api.lose_like.store(true, Ordering::Release);
    assert!(f.client.mini_action(&known, Action::Like).await.is_err());
    let pending = f.client.mini_snapshot().unwrap();
    assert!(pending.view.like.as_ref().unwrap().pending);
    assert!(matches!(
        f.client.mini_action(&pending, Action::Like).await,
        Err(Error::MutationUncertain)
    ));
    f.client
        .mini_action(&pending, Action::RefreshLike)
        .await
        .unwrap();
    let observed = f.client.mini_snapshot().unwrap();
    assert_eq!(observed.view.like.as_ref().unwrap().observed, Some(true));
    assert!(!observed.view.like.as_ref().unwrap().pending);
    assert_eq!(f.api.like_writes.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn mini_seek_volume_and_mode_use_ordinary_validation_and_fences() {
    use cumulus_core::playback::PlaybackMode;
    let f = Fixture::new();
    f.api.trial.store(true, Ordering::Release);
    f.client.play_selection("1", songs()).await.unwrap();
    let first = f.client.mini_snapshot().unwrap();
    assert_eq!(first.view.seek_start, 30.0);
    for action in [
        Action::Seek { seconds: 10.0 },
        Action::Seek { seconds: 65.0 },
        Action::Volume { value: 101.0 },
    ] {
        assert!(f.client.mini_action(&first, action).await.is_err());
    }
    f.client
        .mini_action(&first, Action::Seek { seconds: 45.0 })
        .await
        .unwrap();
    assert!(matches!(
        f.client.mini_action(&first, Action::Stop).await,
        Err(Error::StaleResponse)
    ));
    let current = f.client.mini_snapshot().unwrap();
    f.client
        .mini_action(&current, Action::Volume { value: 12.0 })
        .await
        .unwrap();
    let current = f.client.mini_snapshot().unwrap();
    assert_eq!(current.view.volume, 12.0);
    f.client
        .mini_action(
            &current,
            Action::Mode {
                mode: PlaybackMode::SingleLoop,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.client.mini_snapshot().unwrap().view.mode,
        Some(PlaybackMode::SingleLoop)
    );
}
