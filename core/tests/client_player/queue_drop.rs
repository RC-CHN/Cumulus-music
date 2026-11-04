use super::*;
use sha2::{Digest, Sha256};

fn command(
    rows: &[Value],
    snapshot: &PlayerView,
    track_id: &str,
    track_index: usize,
    before: Option<String>,
) -> PlayerAction {
    PlayerAction::DropPlaylistSongIntoQueue {
        id: "2".into(),
        revision: hex::encode(Sha256::digest(
            serde_json::to_vec(&json!(["2", rows])).unwrap(),
        )),
        track_id: track_id.into(),
        track_index,
        quality: Quality::Lossless,
        expected_queue_revision: snapshot.queue_revision.clone(),
        expected_play_id: snapshot.play_id.clone(),
        before_entry_id: before,
    }
}

#[tokio::test]
async fn verified_playlist_drop_inserts_at_anchor_and_retains_native_transport() {
    let (_dir, api, _, client, _) = fixture();
    let rows = vec![json!({"id":7,"v":-1}), json!({"id":4}), json!({"id":2})];
    *api.playlist_tracks.lock().unwrap() = Some(rows.clone());
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["2".into(), "3".into()],
                index: 0,
                quality: Quality::Lossless,
            },
        )
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 21.0 })
        .await
        .unwrap();
    let before = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let queue = client.player_queue("1").await.unwrap();
    let first = queue.tracks[0].entry_id.clone();
    let third = queue.tracks[1].entry_id.clone();
    api.calls.lock().unwrap().clear();
    for invalid in [
        command(&rows, &before, "4", 0, Some(first.clone())),
        command(&rows, &before, "8", 1, Some(first.clone())),
        command(&rows, &before, "7", 0, Some(first.clone())),
    ] {
        assert!(matches!(
            client.player_action("1", invalid).await,
            Err(Error::InvalidInput(
                "playlist drop row is outside the verified source"
            ))
        ));
    }
    let stale_source = client
        .player_action(
            "1",
            command(&rows[..2], &before, "4", 1, Some(first.clone())),
        )
        .await
        .err()
        .expect("stale source must fail");
    assert!(matches!(stale_source, Error::PlaylistDropSourceChanged));
    let source_issue = cumulus_core::client::ClientError::from(stale_source);
    assert_eq!(source_issue.code, "playlist_drop_source_changed");
    assert!(!source_issue.retryable);
    assert_eq!(
        *api.calls.lock().unwrap(),
        vec!["/api/v6/playlist/detail"; 4],
    );
    api.playlist_cloud_song_id.store(4, Ordering::SeqCst);
    assert!(matches!(
        client
            .player_action("1", command(&rows, &before, "4", 1, Some(first.clone())))
            .await,
        Err(Error::State("playlist drops do not support cloud songs"))
    ));
    assert_eq!(client.player_queue("1").await.unwrap().tracks.len(), 2);
    api.playlist_cloud_song_id.store(0, Ordering::SeqCst);
    api.calls.lock().unwrap().clear();
    let inserted = client
        .player_action("1", command(&rows, &before, "4", 1, Some(first.clone())))
        .await
        .unwrap();
    assert_eq!(
        *api.calls.lock().unwrap(),
        ["/api/v6/playlist/detail", "/api/v3/song/detail"]
    );
    assert_eq!(inserted.play_id, before.play_id);
    assert_eq!(inserted.entry_id, before.entry_id);
    assert_eq!(inserted.state, before.state);
    assert_eq!(inserted.position, Some(21.0));
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(queue.tracks[0].track.id, "4");
    assert_eq!(queue.tracks[1].entry_id, first);
    assert_eq!(queue.tracks[2].entry_id, third);
    api.calls.lock().unwrap().clear();
    let stale_target = client
        .player_action("1", command(&rows, &before, "4", 1, Some(third)))
        .await
        .err()
        .expect("stale target must fail");
    assert!(matches!(stale_target, Error::QueueChanged));
    assert_eq!(
        cumulus_core::client::ClientError::from(stale_target).code,
        "snapshot_changed"
    );
    assert!(api.calls.lock().unwrap().is_empty());
    let moved = client
        .player_action("1", command(&rows, &inserted, "2", 2, None))
        .await
        .unwrap();
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(queue.tracks[2].entry_id, first);
    assert_eq!(moved.play_id, before.play_id);
    assert_eq!(moved.entry_id, before.entry_id);
    assert_eq!(moved.state, before.state);
    assert_eq!(moved.position, Some(21.0));
}

#[tokio::test]
async fn held_playlist_drop_cannot_overwrite_new_playback() {
    let (_dir, api, _, client, _) = fixture();
    let rows = vec![json!({"id":4})];
    *api.playlist_tracks.lock().unwrap() = Some(rows.clone());
    let before = client.play_selection("1", songs("2")).await.unwrap();
    api.hold_id.store(4, Ordering::SeqCst);
    let action = command(&rows, &before, "4", 0, None);
    let pending = {
        let client = client.clone();
        tokio::spawn(async move { client.player_action("1", action).await })
    };
    api.started.notified().await;
    let newer = client.play_selection("1", songs("5")).await.unwrap();
    api.release.notify_one();
    assert!(pending.await.unwrap().is_err());
    assert_eq!(client.player_queue("1").await.unwrap().tracks.len(), 1);
    assert_eq!(newer.track.unwrap().id, "5");
    assert_eq!(
        client.player_queue("1").await.unwrap().tracks[0].track.id,
        "5"
    );
}

fn batch_command(
    rows: &[Value],
    snapshot: &PlayerView,
    selected: &[(usize, &str)],
    before: Option<String>,
) -> PlayerAction {
    PlayerAction::DropPlaylistSongsIntoQueue {
        id: "2".into(),
        revision: hex::encode(Sha256::digest(
            serde_json::to_vec(&json!(["2", rows])).unwrap(),
        )),
        rows: selected
            .iter()
            .map(|&(track_index, track_id)| PlaylistDropRow {
                track_id: track_id.into(),
                track_index,
            })
            .collect(),
        quality: Quality::Lossless,
        expected_queue_revision: snapshot.queue_revision.clone(),
        expected_play_id: snapshot.play_id.clone(),
        before_entry_id: before,
    }
}

#[tokio::test]
async fn batch_drop_keeps_gesture_order_and_original_gap_while_moving_current() {
    let (_dir, api, _, client, _) = fixture();
    let rows = vec![json!({"id":2}), json!({"id":3}), json!({"id":4})];
    *api.playlist_tracks.lock().unwrap() = Some(rows.clone());
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: ["1", "2", "3", "5"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                index: 1,
                quality: Quality::Lossless,
            },
        )
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 22.0 })
        .await
        .unwrap();
    let before = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let original = client.player_queue("1").await.unwrap();
    let anchor = original.tracks[2].entry_id.clone();
    let result = client
        .player_action(
            "1",
            batch_command(
                &rows,
                &before,
                &[(2, "4"), (0, "2"), (1, "3"), (2, "4")],
                Some(anchor.clone()),
            ),
        )
        .await
        .unwrap();
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|row| row.track.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "4", "2", "3", "5"]
    );
    assert_eq!(queue.tracks[2].entry_id, original.tracks[1].entry_id);
    assert_eq!(queue.tracks[3].entry_id, anchor);
    assert_eq!(result.play_id, before.play_id);
    assert_eq!(result.entry_id, before.entry_id);
    assert_eq!(result.state, before.state);
    assert_eq!(result.position, Some(22.0));
}

#[tokio::test]
async fn batch_drop_does_not_commit_a_prefix_after_a_late_invalid_row_or_chunk() {
    let (_dir, api, _, client, _) = fixture();
    let ids = (1..=105).map(|id| id.to_string()).collect::<Vec<_>>();
    let rows = ids.iter().map(|id| json!({"id":id})).collect::<Vec<_>>();
    *api.playlist_tracks.lock().unwrap() = Some(rows.clone());
    let before = client.play_selection("1", songs("1000")).await.unwrap();
    api.calls.lock().unwrap().clear();
    let selected = ids
        .iter()
        .enumerate()
        .map(|(index, id)| (index, id.as_str()))
        .collect::<Vec<_>>();
    let mut invalid = selected.clone();
    invalid[104].0 = 0;
    assert!(matches!(
        client
            .player_action("1", batch_command(&rows, &before, &invalid, None))
            .await,
        Err(Error::InvalidInput(
            "playlist drop row is outside the verified source"
        ))
    ));
    assert_eq!(*api.calls.lock().unwrap(), ["/api/v6/playlist/detail"]);
    api.calls.lock().unwrap().clear();
    api.playlist_cloud_song_id.store(101, Ordering::SeqCst);
    assert!(matches!(
        client
            .player_action("1", batch_command(&rows, &before, &selected, None))
            .await,
        Err(Error::State("playlist drops do not support cloud songs"))
    ));
    assert_eq!(
        *api.calls.lock().unwrap(),
        [
            "/api/v6/playlist/detail",
            "/api/v3/song/detail",
            "/api/v3/song/detail"
        ]
    );
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(queue.revision, before.queue_revision);
    assert_eq!(queue.tracks[0].track.id, "1000");
    assert_eq!(queue.tracks.len(), 1);
}
