use super::*;
use cumulus_core::preferences::desktop_lyrics::Appearance;
#[tokio::test]
async fn desktop_document_is_retired_when_video_owns_output_even_if_audio_later_resumes() {
    let f = Fixture::new();
    let current = f.client.play_selection("1", songs()).await.unwrap();
    let id = current.play_id.unwrap();
    let mut document = f.client.desktop_lyric_document("1", &id).await.unwrap();
    assert_eq!(
        f.client
            .desktop_lyric_frame(&document, &Appearance::default())
            .unwrap()
            .frame
            .lines[0]
            .text,
        "Shared song"
    );
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    assert!(matches!(
        f.client
            .desktop_lyric_frame(&document, &Appearance::default()),
        Err(Error::StaleResponse)
    ));
    assert!(
        f.client
            .desktop_lyric_offset(&mut document, 1.0)
            .await
            .is_err()
    );
    assert!(f.client.desktop_lyric_document("1", &id).await.is_err());
    f.client
        .player_action("1", PlayerAction::Pause { paused: false })
        .await
        .unwrap();
    assert!(
        f.client
            .refresh_desktop_lyric_document(&mut document)
            .await
            .is_err()
    );
    let observed = f
        .client
        .native_shortcut_selection()
        .unwrap()
        .audio_play_id
        .unwrap();
    let new = f
        .client
        .desktop_lyric_document("1", &observed)
        .await
        .unwrap();
    f.client
        .desktop_lyric_frame(&new, &Appearance::default())
        .unwrap();
}
#[tokio::test]
async fn slow_lyric_response_cannot_reappear_over_video_and_empty_failure_does_not_retry() {
    let f = Fixture::new();
    let current = f.client.play_selection("1", songs()).await.unwrap();
    let id = current.play_id.unwrap();
    f.api.hold_lyric.store(true, Ordering::Release);
    let client = f.client.clone();
    let task = tokio::spawn(async move { client.desktop_lyric_document("1", &id).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), f.api.started.notified())
        .await
        .unwrap();
    f.client.video_action("1", open()).await.unwrap();
    f.until(VideoStatus::Playing).await;
    f.api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    let other = f
        .client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["999".into()],
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    f.api.lyric_offline.store(true, Ordering::Release);
    let mut missing = f
        .client
        .desktop_lyric_document("1", other.play_id.as_ref().unwrap())
        .await
        .unwrap();
    let value = f
        .client
        .desktop_lyric_frame(&missing, &Appearance::default())
        .unwrap();
    assert!(value.info.network_failed && value.info.fetch_failed && value.frame.lines.is_empty());
    // A second accidental request would hang at this gate. Local observation
    // must reuse the failed revision until the user explicitly requests retry.
    f.api.hold_lyric.store(true, Ordering::Release);
    for _ in 0..10 {
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            assert!(
                !f.client
                    .refresh_desktop_lyric_document(&mut missing)
                    .await
                    .unwrap()
            );
        })
        .await
        .unwrap();
    }
    assert!(f.api.hold_lyric.load(Ordering::Acquire));
}
