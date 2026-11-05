use super::*;
use cumulus_core::playback::controller::PlaybackStatus;

fn tracks(first: u64) -> Vec<Value> {
    vec![
        json!({"id":11,"name":"范围甲","dt":first}),
        json!({"id":12,"name":"范围乙","dt":90_000}),
        json!({"id":13,"name":"范围丙","dt":180_000}),
        json!({"id":14,"name":"别的歌","dt":120_000}),
    ]
}

async fn scope(client: &MusicClient, order: AlbumDurationOrder) -> (AlbumPlaybackScope, String) {
    let page = client
        .catalog_page("1", CatalogQuery::Album { id: "7".into() }, true)
        .await
        .unwrap()
        .value
        .unwrap();
    let CatalogPageView::Album {
        playback_revision, ..
    } = page.page
    else {
        panic!("expected album");
    };
    (
        AlbumPlaybackScope {
            id: "7".into(),
            revision: playback_revision,
            order,
        },
        page.revision,
    )
}

fn row(ordinals: Vec<usize>, track_index: usize, track_id: &str) -> AlbumPlaybackRow {
    AlbumPlaybackRow {
        visible_ordinals: Some(ordinals),
        track_index,
        track_id: track_id.into(),
    }
}

fn selection(album: AlbumPlaybackScope, row: Option<AlbumPlaybackRow>) -> PlaySelection {
    PlaySelection::AlbumDetail {
        album,
        row,
        quality: Quality::Standard,
    }
}

async fn ids(client: &MusicClient) -> Vec<String> {
    client
        .player_queue("1")
        .await
        .unwrap()
        .tracks
        .into_iter()
        .map(|entry| entry.track.id)
        .collect()
}

#[tokio::test]
async fn filtered_row_uses_bound_visible_order_full_header_uses_fresh_whole_album() {
    let (_dir, api, _session, client, _count) = fixture();
    *api.album_tracks.lock().unwrap() = Some(tracks(180_000));
    let (album, _) = scope(&client, AlbumDurationOrder::Descending).await;
    api.calls.lock().unwrap().clear();

    let result = client
        .play_selection(
            "1",
            selection(album.clone(), Some(row(vec![0, 1, 2], 2, "13"))),
        )
        .await
        .unwrap();
    assert_eq!(result.track.unwrap().id, "13");
    assert_eq!(ids(&client).await, ["11", "13", "12"]);
    assert!(matches!(result.origin, Some(PlayerOrigin::Album { id }) if id == "7"));
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|path| path.starts_with("/api/album/"))
    );

    // The full resource applies the chosen order to the newly read metadata,
    // not the duration ordering present in the cached table.
    *api.album_tracks.lock().unwrap() = Some(tracks(20_000));
    api.calls.lock().unwrap().clear();
    let full = client
        .play_selection("1", selection(album, None))
        .await
        .unwrap();
    assert_eq!(full.track.unwrap().id, "13");
    assert_eq!(ids(&client).await, ["13", "14", "12", "11"]);
    let calls = api.calls.lock().unwrap();
    assert_eq!(
        &calls[..2],
        ["/api/album/v3/detail", "/api/album/privilege"]
    );
}

#[tokio::test]
async fn whole_append_preserves_paused_identity_and_stable_order_without_the_filter() {
    let (_dir, api, _session, client, _count) = fixture();
    *api.album_tracks.lock().unwrap() = Some(tracks(180_000));
    let (album, _) = scope(&client, AlbumDurationOrder::Ascending).await;
    client.play_selection("1", songs("1000")).await.unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 23.0 })
        .await
        .unwrap();
    let before = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let after = client
        .player_action(
            "1",
            PlayerAction::QueueAlbumDetail {
                album,
                quality: Quality::Standard,
                expected_queue_revision: before.queue_revision.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(after.state, PlaybackStatus::Paused);
    assert_eq!(
        (after.play_id, after.entry_id, after.position),
        (before.play_id, before.entry_id, before.position)
    );
    assert_eq!(ids(&client).await, ["1000", "12", "14", "11", "13"]);
}

#[tokio::test]
async fn same_ids_new_durations_during_hydration_reject_old_row_and_foreign_ordinals() {
    let (_dir, api, _session, client, _count) = fixture();
    *api.album_tracks.lock().unwrap() = Some(tracks(180_000));
    let (album, id_revision) = scope(&client, AlbumDurationOrder::Ascending).await;
    let before = client.play_selection("1", songs("1000")).await.unwrap();
    api.calls.lock().unwrap().clear();

    for malformed in [
        row(vec![0, 1], 2, "13"),
        row(vec![0, 0, 2], 2, "13"),
        row(vec![0, 2], 2, "14"),
    ] {
        assert!(matches!(
            client
                .play_selection("1", selection(album.clone(), Some(malformed)))
                .await,
            Err(Error::QueueChanged | Error::InvalidInput(_))
        ));
    }
    assert!(api.calls.lock().unwrap().is_empty());

    api.hold_id.store(12, Ordering::SeqCst);
    let pending = tokio::spawn({
        let client = client.clone();
        let album = album.clone();
        async move {
            client
                .play_selection("1", selection(album, Some(row(vec![0, 1, 2], 2, "13"))))
                .await
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), api.started.notified())
        .await
        .unwrap();
    *api.album_tracks.lock().unwrap() = Some(tracks(25_000));
    let (new, new_ids) = scope(&client, AlbumDurationOrder::Ascending).await;
    assert_eq!(
        id_revision, new_ids,
        "drag keeps the original ID-only revision"
    );
    assert_ne!(
        album.revision, new.revision,
        "playback receipt includes durations"
    );
    api.release.notify_one();
    assert!(matches!(
        pending.await.unwrap(),
        Err(Error::QueueChanged | Error::StaleResponse)
    ));
    assert_eq!(ids(&client).await, ["1000"]);
    assert_eq!(
        client.player_snapshot("1").await.unwrap().play_id,
        before.play_id
    );
}

#[tokio::test]
async fn replacing_double_click_clears_to_only_clicked_song() {
    let (_dir, api, _session, client, _count) = fixture();
    *api.album_tracks.lock().unwrap() = Some(tracks(180_000));
    let (album, _) = scope(&client, AlbumDurationOrder::Ascending).await;
    client.play_selection("1", songs("1000")).await.unwrap();
    let started = client
        .play_selection(
            "1",
            PlaySelection::SongRowDoubleClick {
                selection: SongRowSelection::AlbumDetail {
                    album,
                    row: row(vec![0, 1, 2], 0, "11"),
                    quality: Quality::Standard,
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(started.track.unwrap().id, "11");
    assert_eq!(ids(&client).await, ["11"]);
}
