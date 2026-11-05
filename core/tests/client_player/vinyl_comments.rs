use super::*;

#[tokio::test]
async fn embedded_vinyl_comments_preserve_official_offset_only_wire_and_group_gate() {
    let (_dir, api, _, client, _) = fixture();
    api.vinyl_comment_group.store(2, Ordering::SeqCst);
    let play = client.play_selection("1", songs("1")).await.unwrap();
    let play_id = play.play_id.as_deref().unwrap();
    assert!(!client.player_vinyl_comments_available("1").await.unwrap());
    assert!(client.player_vinyl_comments("1", play_id, 0).await.is_err());
    assert!(api.vinyl_comment_calls.lock().unwrap().is_empty());
    api.vinyl_comment_group.store(1, Ordering::SeqCst);
    assert!(client.player_vinyl_comments_available("1").await.unwrap());
    let first = client.player_vinyl_comments("1", play_id, 0).await.unwrap();
    assert_eq!(first.value.hot.len(), 5);
    assert!(first.value.more_hot);
    assert_eq!(first.value.latest.len(), 2);
    let offset = first.value.next_offset.unwrap();
    assert_eq!(offset, 2);
    let second = client
        .player_vinyl_comments("1", play_id, offset)
        .await
        .unwrap();
    assert_eq!(second.value.latest[0].id, "3");
    assert!(second.value.hot.is_empty() && !second.value.more_hot);
    assert!(second.value.next_offset.is_none());
    for ((path, wire), offset) in api.vinyl_comment_calls.lock().unwrap().iter().zip([0, 2]) {
        assert_eq!(path, "/api/v1/resource/comments/R_SO_4_1");
        assert_eq!(
            *wire,
            json!({"limit":60,"offset":offset,"beforeTime":0,"commentId":"0",
            "compareUserLocation":false,"composeConcert":false,"markReplied":false,
            "forceFlatComment":false,"showInner":false})
        );
    }
    assert_eq!(
        client
            .player_snapshot("1")
            .await
            .unwrap()
            .play_id
            .as_deref(),
        Some(play_id)
    );
    let voice = client
        .play_selection("1", PlaySelection::Voice { id: "81".into() })
        .await
        .unwrap();
    assert!(
        client
            .player_vinyl_comments("1", voice.play_id.as_deref().unwrap(), 0)
            .await
            .is_err()
    );
    assert_eq!(api.vinyl_comment_calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn embedded_vinyl_comments_reject_reply_after_new_play_or_credential_revision() {
    let (_dir, api, _, client, _) = fixture();
    api.vinyl_comment_group.store(1, Ordering::SeqCst);
    let original = client
        .play_selection("1", songs("1"))
        .await
        .unwrap()
        .play_id
        .unwrap();
    for swap_track in [false, true] {
        let client_for_read = client.clone();
        let play_id = original.clone();
        api.vinyl_comment_hold.store(1, Ordering::SeqCst);
        let delayed = tokio::spawn(async move {
            client_for_read
                .player_vinyl_comments("1", &play_id, 0)
                .await
        });
        api.vinyl_comment_started.notified().await;
        if swap_track {
            client.play_selection("1", songs("2")).await.unwrap();
        } else {
            api.vinyl_comment_group.store(2, Ordering::SeqCst);
        }
        api.vinyl_comment_release.notify_one();
        assert!(matches!(delayed.await.unwrap(), Err(Error::StaleResponse)));
        api.vinyl_comment_group.store(1, Ordering::SeqCst);
    }
    assert!(matches!(
        client.player_vinyl_comments("1", &original, 0).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.vinyl_comment_calls.lock().unwrap().len(), 2);
}
