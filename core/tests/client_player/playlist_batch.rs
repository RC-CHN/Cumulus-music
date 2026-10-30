use super::*;
use sha2::{Digest, Sha256};

fn revision(tracks: &[Value]) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(&json!(["2", tracks])).unwrap(),
    ))
}
fn selection(ids: Vec<String>, revision: String) -> PlaySelection {
    PlaySelection::PlaylistBatch {
        id: "2".into(),
        ids,
        revision,
        quality: Quality::Lossless,
    }
}
#[tokio::test]
async fn batch_over_500_uses_playlist_order_not_click_order_and_keeps_all_selected_rows() {
    let (_dir, api, _, client, _) = fixture();
    let tracks: Vec<_> = (1..=601).map(|id| json!({"id":id})).collect();
    *api.playlist_tracks.lock().unwrap() = Some(tracks.clone());
    let mut ids: Vec<_> = (1..=601).rev().map(|id| id.to_string()).collect();
    ids.push("4".into());
    let played = client
        .play_selection("1", selection(ids, revision(&tracks)))
        .await
        .unwrap();
    assert_eq!(played.track.unwrap().id, "1");
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|v| v.track.id.clone())
            .collect::<Vec<_>>(),
        (1..=601).map(|id| id.to_string()).collect::<Vec<_>>()
    );
}
#[tokio::test]
async fn selected_queue_rows_preserve_paused_current_and_reject_changed_snapshot() {
    let (_dir, api, _, client, _) = fixture();
    let tracks: Vec<_> = (1..=205).map(|id| json!({"id":id})).collect();
    let before = client.play_selection("1", songs("1000")).await.unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 21.0 })
        .await
        .unwrap();
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let command = || PlayerAction::QueuePlaylistBatch {
        id: "2".into(),
        ids: vec!["205".into(), "3".into(), "1".into()],
        revision: revision(&tracks),
        quality: Quality::Lossless,
        expected_queue_revision: before.queue_revision.clone(),
    };
    let next = client.player_action("1", command()).await.unwrap();
    assert_eq!(next.play_id, paused.play_id);
    assert_eq!(next.position, paused.position);
    assert_eq!(next.state, paused.state);
    assert_eq!(
        client
            .player_queue("1")
            .await
            .unwrap()
            .tracks
            .iter()
            .map(|v| v.track.id.as_str())
            .collect::<Vec<_>>(),
        ["1000", "1", "3", "205"]
    );
    *api.playlist_tracks.lock().unwrap() = Some(vec![json!({"id":1})]);
    assert!(matches!(
        client
            .play_selection("1", selection(vec!["1".into()], revision(&tracks)))
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(client.player_queue("1").await.unwrap().tracks.len(), 4);
}
#[tokio::test]
async fn batch_rejects_local_absent_empty_and_oversized_selection_without_replacing_queue() {
    let (_dir, api, _, client, _) = fixture();
    client.play_selection("1", songs("1000")).await.unwrap();
    let tracks = vec![json!({"id":1,"v":-1}), json!({"id":2})];
    *api.playlist_tracks.lock().unwrap() = Some(tracks.clone());
    for ids in [
        vec![],
        vec!["1".into()],
        vec!["3".into()],
        vec!["2".into(); 10001],
    ] {
        assert!(
            client
                .play_selection("1", selection(ids, revision(&tracks)))
                .await
                .is_err()
        );
        assert_eq!(
            client.player_queue("1").await.unwrap().tracks[0].track.id,
            "1000"
        );
    }
}
#[tokio::test]
async fn batch_metadata_returning_after_account_switch_cannot_load_previous_selection() {
    let (_dir, api, session, client, _) = fixture();
    let tracks: Vec<_> = (1..=205).map(|id| json!({"id":id})).collect();
    api.hold_id.store(1, Ordering::SeqCst);
    let c = client.clone();
    let pending = tokio::spawn(async move {
        c.play_selection("1", selection(vec!["1".into()], revision(&tracks)))
            .await
    });
    api.started.notified().await;
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    api.release.notify_one();
    assert!(pending.await.unwrap().is_err());
    assert!(client.player_queue("2").await.unwrap().tracks.is_empty());
}
