use super::*;
use cumulus_core::{
    client::dto::{MixedPlaylistDropRow, PlayerAction, PlayerView},
    membership::Quality,
    playback::controller::PlaybackStatus,
};

fn append(view: &PlayerView, revision: &str, track: Option<(&str, bool)>) -> PlayerAction {
    PlayerAction::QueueMixedPlaylist {
        id: "73".into(),
        revision: revision.into(),
        track: track.map(|(id, local)| MixedPlaylistStart {
            id: id.into(),
            local,
        }),
        quality: Quality::Standard,
        expected_queue_revision: view.queue_revision.clone(),
    }
}

fn drop_tracks(
    view: &PlayerView,
    revision: &str,
    rows: &[(usize, &str, bool)],
    before_entry_id: Option<String>,
) -> PlayerAction {
    PlayerAction::DropMixedPlaylistSongsIntoQueue {
        id: "73".into(),
        revision: revision.into(),
        rows: rows
            .iter()
            .map(|&(track_index, track_id, local)| MixedPlaylistDropRow {
                track_id: track_id.into(),
                track_index,
                local,
            })
            .collect(),
        quality: Quality::Standard,
        expected_queue_revision: view.queue_revision.clone(),
        expected_play_id: view.play_id.clone(),
        before_entry_id,
    }
}

async fn order(client: &MusicClient) -> Vec<String> {
    client
        .player_queue("1")
        .await
        .unwrap()
        .tracks
        .into_iter()
        .map(|entry| entry.track.id)
        .collect()
}

fn same_paused_audio(before: &PlayerView, after: &PlayerView) {
    assert_eq!(after.play_id, before.play_id);
    assert_eq!(after.entry_id, before.entry_id);
    assert_eq!(after.position, before.position);
    assert_eq!(after.state, PlaybackStatus::Paused);
}

#[tokio::test]
async fn typed_mixed_next_whole_and_drops_preserve_the_paused_native_source() {
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    for name in ["a", "b"] {
        std::fs::write(media.join(format!("{name}.wav")), name.as_bytes()).unwrap();
    }
    let server = Arc::new(Server::new());
    let (client, session, library) = open(&temp.path().join("profile"), server.clone(), Some(9));
    library.scan(&media).unwrap();
    let rows = refs(&client).await;
    let (a, b) = (&rows[0], &rows[1]);
    let revision = client
        .mutate_playlist_local("1", "73", &project(&session, &server).revision, add(&rows))
        .await
        .unwrap()
        .value
        .membership
        .revision;
    let empty = client.player_snapshot("1").await.unwrap();
    client
        .player_action("1", append(&empty, &revision, Some((&b.id, true))))
        .await
        .unwrap();
    let playing = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let view = client.player_snapshot("1").await.unwrap();
            if view.state == PlaybackStatus::Playing {
                break view;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(playing.resource_kind, Some(PlayerResourceKind::Local));
    let first = client.player_queue("1").await.unwrap();
    assert_eq!(first.tracks.len(), 1);
    assert_eq!(first.tracks[0].track.id, b.id);
    assert!(first.tracks[0].track.local && first.tracks[0].local_menu);
    assert_eq!(
        serde_json::to_value(&first.tracks[0].origin).unwrap(),
        json!({"kind": "playlist", "id": "73"})
    );
    assert_eq!(first.tracks[0].source_label.as_deref(), Some("Synthetic"));
    assert!(server.songs.lock().unwrap().is_empty());
    client
        .player_action("1", PlayerAction::Seek { seconds: 21.0 })
        .await
        .unwrap();
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    assert_eq!(paused.position, Some(21.0));
    let next = client
        .player_action("1", append(&paused, &revision, Some((&a.id, true))))
        .await
        .unwrap();
    same_paused_audio(&paused, &next);
    assert_eq!(order(&client).await, [b.id.as_str(), a.id.as_str()]);
    assert!(server.songs.lock().unwrap().is_empty());

    let remote = client
        .player_action("1", append(&next, &revision, Some(("42", false))))
        .await
        .unwrap();
    same_paused_audio(&paused, &remote);
    assert_eq!(order(&client).await, [b.id.as_str(), "42", a.id.as_str()]);
    let all = client
        .player_action("1", append(&remote, &revision, None))
        .await
        .unwrap();
    same_paused_audio(&paused, &all);
    assert_eq!(
        order(&client).await,
        [b.id.as_str(), a.id.as_str(), "11", "42"]
    );

    let before = client.player_queue("1").await.unwrap().tracks[0]
        .entry_id
        .clone();
    let leading = client
        .player_action(
            "1",
            drop_tracks(
                &all,
                &revision,
                &[(4, "42", false), (0, &a.id, true)],
                Some(before),
            ),
        )
        .await
        .unwrap();
    same_paused_audio(&paused, &leading);
    assert_eq!(
        order(&client).await,
        [b.id.as_str(), "42", a.id.as_str(), "11"]
    );
    let before = client.player_queue("1").await.unwrap().tracks[1]
        .entry_id
        .clone();
    let exact = client
        .player_action(
            "1",
            drop_tracks(&leading, &revision, &[(2, "11", false)], Some(before)),
        )
        .await
        .unwrap();
    same_paused_audio(&paused, &exact);
    assert_eq!(
        order(&client).await,
        [b.id.as_str(), "11", "42", a.id.as_str()]
    );

    assert!(matches!(
        client
            .player_action("1", append(&exact, &revision, Some((&a.id, false))))
            .await,
        Err(Error::State(_))
    ));
    assert!(matches!(
        client
            .player_action(
                "1",
                drop_tracks(&exact, &revision, &[(3, "77", true)], None)
            )
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        client
            .player_action("1", drop_tracks(&exact, "stale", &[(4, "42", false)], None))
            .await,
        Err(Error::PlaylistDropSourceChanged)
    ));
    assert!(matches!(
        client
            .player_action("1", append(&remote, &revision, None))
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(
        *server.songs.lock().unwrap(),
        [vec!["42"], vec!["11", "42"], vec!["42"], vec!["11"]]
    );
    assert_eq!(std::fs::read(media.join("a.wav")).unwrap(), b"a");
    assert_eq!(std::fs::read(media.join("b.wav")).unwrap(), b"b");
}

#[tokio::test]
async fn a_membership_edit_while_catalog_hydrates_does_not_commit_the_old_mix() {
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let file = media.join("track.wav");
    std::fs::write(&file, b"track").unwrap();
    let server = Arc::new(Server::new());
    let (client, session, library) = open(&temp.path().join("profile"), server.clone(), Some(9));
    let client = Arc::new(client);
    library.scan(&media).unwrap();
    let row = refs(&client).await.remove(0);
    let revision = client
        .mutate_playlist_local(
            "1",
            "73",
            &project(&session, &server).revision,
            add(std::slice::from_ref(&row)),
        )
        .await
        .unwrap()
        .value
        .membership
        .revision;
    let initial = client.player_snapshot("1").await.unwrap();
    let action = append(&initial, &revision, None);
    server
        .hold_songs
        .store(true, std::sync::atomic::Ordering::Release);
    let loading = {
        let client = client.clone();
        tokio::spawn(async move { client.player_action("1", action).await })
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        server.songs_started.notified(),
    )
    .await
    .unwrap();
    let removed = client
        .mutate_playlist_local("1", "73", &revision, remove(std::slice::from_ref(&row)))
        .await
        .unwrap()
        .value;
    assert!(removed.changed);
    server.release_songs.notify_one();
    assert!(matches!(loading.await.unwrap(), Err(Error::QueueChanged)));
    assert!(order(&client).await.is_empty());
    let after = client.player_snapshot("1").await.unwrap();
    assert_eq!(after.play_id, initial.play_id);
    assert_eq!(after.queue_revision, initial.queue_revision);
    assert_eq!(std::fs::read(file).unwrap(), b"track");
}
