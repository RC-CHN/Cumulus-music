use super::*;
#[tokio::test]
async fn native_song_like_roundtrip_rejects_unknown_stale_and_duplicate_observations() {
    let f = Fixture::new();
    assert!(f.client.native_like_selection().unwrap().is_none());
    f.client.play_selection("1", songs()).await.unwrap();
    let selection = f.client.native_like_selection().unwrap().unwrap();
    let unknown = f.client.native_like_snapshot(&selection).unwrap();
    assert_eq!(unknown.observed, None);
    assert!(f.client.native_like_toggle(&unknown).await.is_err());
    assert!(f.api.like_writes.lock().unwrap().is_empty());
    f.client.liked_ids().await.unwrap();
    let view = f.client.native_like_snapshot(&selection).unwrap();
    assert_eq!(view.observed, Some(false));
    f.client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    f.client.native_like_toggle(&view).await.unwrap();
    assert!(matches!(
        f.client.native_like_toggle(&view).await,
        Err(Error::StaleResponse)
    ));
    let liked = f.client.native_like_snapshot(&selection).unwrap();
    assert_eq!(liked.observed, Some(true));
    f.client.native_like_toggle(&liked).await.unwrap();
    assert_eq!(
        f.client.native_like_snapshot(&selection).unwrap().observed,
        Some(false)
    );
    let writes = f.api.like_writes.lock().unwrap();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0]["trackId"], "7");
    assert_eq!(writes[0]["userid"], "9");
    assert_eq!(writes[0]["like"], true);
    assert_eq!(writes[1]["like"], false);
}
#[tokio::test]
async fn native_song_like_lost_reply_is_reconciled_without_replay() {
    let f = Fixture::new();
    f.client.play_selection("1", songs()).await.unwrap();
    let selection = f.client.native_like_selection().unwrap().unwrap();
    let view = f.client.native_like_refresh(&selection).await.unwrap();
    f.api.lose_like.store(true, Ordering::Release);
    assert!(f.client.native_like_toggle(&view).await.is_err());
    let pending = f.client.native_like_snapshot(&selection).unwrap();
    assert!(pending.pending);
    assert!(matches!(
        f.client.native_like_toggle(&pending).await,
        Err(Error::MutationUncertain)
    ));
    let confirmed = f.client.native_like_refresh(&selection).await.unwrap();
    assert_eq!(confirmed.observed, Some(true));
    assert!(!confirmed.pending);
    assert_eq!(f.api.like_writes.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn native_like_identity_excludes_mv_and_changes_on_queue_or_account_replacement() {
    let f = Fixture::new();
    f.client.play_selection("1", songs()).await.unwrap();
    let selection = f.client.native_like_selection().unwrap().unwrap();
    let view = f.client.native_like_refresh(&selection).await.unwrap();
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    assert!(f.client.native_like_selection().unwrap().is_none());
    assert!(matches!(
        f.client.native_like_toggle(&view).await,
        Err(Error::StaleResponse)
    ));
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
        f.client.native_like_toggle(&view).await,
        Err(Error::StaleResponse)
    ));
    let selection = f.client.native_like_selection().unwrap().unwrap();
    let view = f.client.native_like_refresh(&selection).await.unwrap();
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.client.native_like_toggle(&view).await,
        Err(Error::StaleResponse)
    ));
    assert!(f.api.like_writes.lock().unwrap().is_empty());
}
