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
        command(&rows[..2], &before, "4", 1, Some(first.clone())),
    ] {
        assert!(matches!(
            client.player_action("1", invalid).await,
            Err(Error::QueueChanged)
        ));
    }
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
    assert!(matches!(
        client
            .player_action("1", command(&rows, &before, "4", 1, Some(third)))
            .await,
        Err(Error::QueueChanged)
    ));
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
