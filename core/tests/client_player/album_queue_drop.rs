use super::*;
use cumulus_core::{
    catalog::details::CatalogQuery,
    playback::{PlaybackMode, controller::PlaybackStatus},
};

fn drop(
    view: &PlayerView,
    revision: &str,
    before: Option<String>,
    rows: &[[usize; 2]],
) -> PlayerAction {
    serde_json::from_value(json!({
        "kind":"drop_album_songs_into_queue","id":"7","revision":revision,
        "rows":rows.iter().map(|[index,id]|json!({"track_id":id.to_string(),"track_index":index}))
            .collect::<Vec<_>>(),
        "quality":"standard","before_entry_id":before,"expected_play_id":view.play_id,
        "expected_queue_revision":view.queue_revision
    }))
    .unwrap()
}

fn album_song(id: u32, label: &str) -> Value {
    json!({"id":id,"name":label,"dt":120000,"al":{"id":7,"name":"长专辑"},
        "authorization":"synthetic-private-metadata-never-queued"})
}

#[tokio::test]
async fn album_drag_resolves_native_order_without_requests_or_replacing_paused_playback() {
    let (directory, api, _, client, _) = fixture();
    *api.album_tracks.lock().unwrap() = Some(vec![
        album_song(2, "专辑中的当前歌曲"),
        album_song(4, "专辑先显示"),
        album_song(5, "专辑后显示"),
    ]);
    let album = client
        .catalog_page("1", CatalogQuery::Album { id: "7".into() }, true)
        .await
        .unwrap()
        .value
        .unwrap();
    // Isolate the mutation's transport behavior from the normal next-song
    // tick. The real desktop proof independently checks the new neighbor.
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::SingleLoop,
            },
        )
        .await
        .unwrap();
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: ["1", "2", "3"].map(str::to_owned).to_vec(),
                index: 1,
                quality: Quality::Standard,
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
    assert_eq!(before.mode, PlaybackMode::SingleLoop);
    let queue_before = client.player_queue("1").await.unwrap();
    let anchor = Some(queue_before.tracks[2].entry_id.clone());
    api.calls.lock().unwrap().clear();
    let after = client
        .player_action(
            "1",
            drop(&before, &album.revision, anchor, &[[2, 5], [0, 2], [1, 4]]),
        )
        .await
        .unwrap();
    assert_eq!(after.state, PlaybackStatus::Paused);
    assert_eq!(after.play_id, before.play_id);
    assert_eq!(after.entry_id, before.entry_id);
    assert_eq!(after.position, Some(22.0));
    assert_eq!(after.position, before.position);
    assert!(after.origin.is_none());
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(queue.current, queue_before.current);
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|entry| entry.track.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "5", "2", "4", "3"]
    );
    assert_eq!(queue.tracks[2].entry_id, queue_before.tracks[1].entry_id);
    assert_eq!(queue.tracks[1].track.name, "专辑后显示");
    assert_eq!(
        queue.tracks[2].track.name,
        queue_before.tracks[1].track.name
    );
    assert!(
        api.calls.lock().unwrap().is_empty(),
        "dropping uses native page metadata only"
    );

    let persisted = cumulus_core::playback::PlaybackQueue::new(
        Some(Arc::new(
            cumulus_core::storage::StateStore::open(directory.path()).unwrap(),
        )),
        "9",
    )
    .unwrap()
    .snapshot();
    for i in [1, 3] {
        assert_eq!(
            persisted.items[i].source,
            json!({"kind":"album","id":"7","scene":"album"})
        );
        assert!(persisted.items[i].resource["trial_mode"].is_null());
        assert!(
            !persisted.items[i]
                .resource
                .to_string()
                .contains("synthetic-private")
        );
    }
    assert_ne!(persisted.items[2].source["kind"], "album");
    assert!(matches!(
        client
            .player_action("1", drop(&before, &album.revision, None, &[[2, 5]]))
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(
        client.player_queue("1").await.unwrap().revision,
        after.queue_revision
    );
    assert!(api.calls.lock().unwrap().is_empty());

    let selected = client
        .player_action(
            "1",
            PlayerAction::Select {
                entry_id: queue.tracks[1].entry_id.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(selected.origin).unwrap(),
        json!({"kind":"album","id":"7"})
    );
    assert!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|path| path == "/api/song/enhance/player/url/v1")
    );
}

#[tokio::test]
async fn album_drag_rejects_wrong_ordinals_unusable_rows_and_old_account_or_source_atomically() {
    let (_, api, session, client, _) = fixture();
    let rows = vec![
        album_song(4, "可用"),
        json!({"id":5,"name":"网盘混入"}),
        json!({"id":6}),
    ];
    *api.album_tracks.lock().unwrap() = Some(rows.clone());
    *api.album_privileges.lock().unwrap() = vec![json!({"id":5,"cs":true})];
    let query = CatalogQuery::Album { id: "7".into() };
    let album = client
        .catalog_page("1", query.clone(), true)
        .await
        .unwrap()
        .value
        .unwrap();
    let view = client.play_selection("1", songs("1")).await.unwrap();
    api.calls.lock().unwrap().clear();
    for invalid in [[[0, 4], [1, 5]], [[0, 4], [2, 6]], [[1, 4], [0, 4]]] {
        assert!(matches!(
            client
                .player_action("1", drop(&view, &album.revision, None, &invalid))
                .await,
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(
            client.player_queue("1").await.unwrap().revision,
            view.queue_revision
        );
    }
    assert!(api.calls.lock().unwrap().is_empty());
    *api.album_tracks.lock().unwrap() = Some(rows.into_iter().rev().collect());
    client.catalog_page("1", query, true).await.unwrap();
    api.calls.lock().unwrap().clear();
    assert!(matches!(
        client
            .player_action("1", drop(&view, &album.revision, None, &[[0, 4]]))
            .await,
        Err(Error::AlbumDropSourceChanged)
    ));
    assert_eq!(
        client.player_queue("1").await.unwrap().revision,
        view.queue_revision
    );
    assert!(api.calls.lock().unwrap().is_empty());
    let token = session.token().unwrap();
    session
        .adopt(api.clone(), json!({"userId":11}), &token)
        .unwrap();
    let other = client.play_selection("2", songs("1")).await.unwrap();
    assert!(matches!(
        client
            .player_action("2", drop(&other, &album.revision, None, &[[0, 4]]))
            .await,
        Err(Error::AlbumDropSourceChanged)
    ));
    assert_eq!(
        client.player_queue("2").await.unwrap().revision,
        other.queue_revision
    );
}
