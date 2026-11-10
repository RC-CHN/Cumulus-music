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
    assert_eq!(result.origin_name.as_deref(), Some("长专辑"));
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
    assert_eq!(full.origin_name.as_deref(), Some("长专辑"));
    assert_eq!(ids(&client).await, ["13", "14", "12", "11"]);
    let calls = api.calls.lock().unwrap();
    assert_eq!(
        &calls[..2],
        ["/api/album/v3/detail", "/api/album/privilege"]
    );
}

#[tokio::test]
async fn popularity_orders_cached_rows_and_fresh_album_and_retires_same_id_score_changes() {
    let (_dir, api, _session, client, _count) = fixture();
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: cumulus_core::playback::PlaybackMode::SingleLoop,
            },
        )
        .await
        .unwrap();
    let mut songs = tracks(180_000);
    for (song, score) in songs
        .iter_mut()
        .zip([json!(25), json!(96.0), json!(96), json!("99")])
    {
        song["pop"] = score;
    }
    *api.album_tracks.lock().unwrap() = Some(songs.clone());
    let (album, id_revision) = scope(&client, AlbumTrackOrder::PopularityDescending).await;
    let cached = client
        .catalog_page("1", CatalogQuery::Album { id: "7".into() }, false)
        .await
        .unwrap()
        .value
        .unwrap();
    let CatalogPageView::Album { tracks, .. } = cached.page else {
        panic!("expected album")
    };
    assert_eq!(
        tracks
            .iter()
            .map(|row| row.album_popularity)
            .collect::<Vec<_>>(),
        [Some(25), Some(96), Some(96), Some(0)]
    );
    assert_eq!(
        serde_json::to_value(&tracks[1]).unwrap()["albumPopularity"],
        96
    );
    api.calls.lock().unwrap().clear();

    let selected = client
        .play_selection(
            "1",
            selection(album.clone(), Some(row(vec![0, 2, 3], 0, "11"))),
        )
        .await
        .unwrap();
    assert_eq!(selected.track.unwrap().id, "11");
    assert_eq!(ids(&client).await, ["13", "11", "14"]);
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|path| path.starts_with("/api/album/"))
    );

    // A header action reads the full album anew; stale on-screen popularity
    // must not decide the new list's order.
    songs[0]["pop"] = json!(100);
    *api.album_tracks.lock().unwrap() = Some(songs);
    api.calls.lock().unwrap().clear();
    let full = client
        .play_selection("1", selection(album.clone(), None))
        .await
        .unwrap();
    assert_eq!(full.track.unwrap().id, "11");
    assert_eq!(ids(&client).await, ["11", "12", "13", "14"]);
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|path| path.starts_with("/api/album/"))
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["/api/album/v3/detail", "/api/album/privilege"]
    );

    let (updated, same_ids) = scope(&client, AlbumTrackOrder::PopularityAscending).await;
    assert_eq!(same_ids, id_revision);
    assert_ne!(updated.revision, album.revision);
    api.calls.lock().unwrap().clear();
    assert!(matches!(
        client
            .play_selection("1", selection(album, Some(row(vec![0, 2, 3], 2, "13"))))
            .await,
        Err(Error::QueueChanged)
    ));
    assert!(api.calls.lock().unwrap().is_empty());

    let ascending = client
        .play_selection("1", selection(updated, None))
        .await
        .unwrap();
    assert_eq!(ascending.track.unwrap().id, "14");
    assert_eq!(ids(&client).await, ["14", "12", "13", "11"]);
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
async fn peer_same_ids_new_durations_during_hydration_reject_old_row_and_foreign_ordinals() {
    let (dir, api, _session, client, _count) = fixture();
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
    // An independent native instance commits to the same account-owned DB
    // through another SQLite connection while the first hydration is held.
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let peer_session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    peer_session
        .adopt(
            api.clone(),
            json!({"userId":9}),
            &peer_session.token().unwrap(),
        )
        .unwrap();
    let peer = MusicClient::with_session(peer_session, registry);
    *api.album_tracks.lock().unwrap() = Some(tracks(25_000));
    let (new, new_ids) = scope(&peer, AlbumDurationOrder::Ascending).await;
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
async fn album_double_click_replaces_with_sorted_visible_rows_or_inserts_only_selected_song() {
    for replace_queue in [true, false] {
        let (_dir, api, _session, client, _count) = fixture();
        *api.album_tracks.lock().unwrap() = Some(tracks(180_000));
        let (album, _) = scope(&client, AlbumDurationOrder::Ascending).await;
        if !replace_queue {
            client
                .queue_click_preferences_command(
                    "1",
                    QueueClickPreferencesCommand::Change {
                        expected_revision: "0".into(),
                        replace_queue,
                    },
                )
                .unwrap();
        }
        let seed = client
            .play_selection(
                "1",
                PlaySelection::Songs {
                    ids: vec!["1000".into(), "1001".into()],
                    index: 0,
                    quality: Quality::Standard,
                },
            )
            .await
            .unwrap();
        let before = client.player_queue("1").await.unwrap();
        api.calls.lock().unwrap().clear();

        let started = client
            .play_selection(
                "1",
                PlaySelection::SongRowDoubleClick {
                    selection: SongRowSelection::AlbumDetail {
                        album,
                        row: row(vec![0, 1, 2], 2, "13"),
                        quality: Quality::Standard,
                    },
                },
            )
            .await
            .unwrap();
        assert_eq!(started.track.unwrap().id, "13");
        assert_eq!(started.state, PlaybackStatus::Playing);
        assert_ne!(started.play_id, seed.play_id);
        assert!(matches!(started.origin, Some(PlayerOrigin::Album { id }) if id == "7"));
        assert!(
            !api.calls
                .lock()
                .unwrap()
                .iter()
                .any(|path| path.starts_with("/api/album/"))
        );

        let after = client.player_queue("1").await.unwrap();
        let actual: Vec<_> = after
            .tracks
            .iter()
            .map(|row| row.track.id.as_str())
            .collect();
        let (expected, selected) = if replace_queue {
            // Ascending duration, with a service-order tie and a filtered-out row.
            (["12", "11", "13"], 2)
        } else {
            // Retained entries on both sides of the newly inserted selected song.
            assert_eq!(after.tracks[0].entry_id, before.tracks[0].entry_id);
            assert_eq!(after.tracks[2].entry_id, before.tracks[1].entry_id);
            (["1000", "13", "1001"], 1)
        };
        assert_eq!(actual, expected, "replace_queue={replace_queue}");
        assert_eq!(
            after.current.as_ref(),
            Some(&after.tracks[selected].entry_id)
        );
        assert_eq!(started.entry_id, after.current);
    }
}

#[tokio::test]
async fn older_recovery_snapshots_upgrade_without_fresh_album_request() {
    for legacy in ["catalog-detail-pages-v1", "catalog-album-pages-v2"] {
        check_cached_album_upgrade(legacy).await;
    }
}

async fn check_cached_album_upgrade(legacy: &str) {
    let (dir, api, _session, client, _count) = fixture();
    *api.album_tracks.lock().unwrap() = Some(tracks(180_000));
    let (album, _) = scope(&client, AlbumDurationOrder::Descending).await;

    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    store
        .transaction(|tx| {
            let mut entries = tx.values("9", "catalog-album-pages-v3")?;
            assert_eq!(entries.len(), 1);
            let (key, page) = entries.pop_first().unwrap();
            tx.put("9", legacy, &key, &page)?;
            tx.delete("9", "catalog-album-pages-v3", &key)?;
            tx.delete("9", "catalog-album-playback-marker-v2", &key)?;
            tx.put(
                "9",
                "catalog-album-playback-marker-v1",
                &key,
                &json!("legacy-duration-marker"),
            )
        })
        .unwrap();

    api.calls.lock().unwrap().clear();
    let recovered = client
        .catalog_page("1", CatalogQuery::Album { id: "7".into() }, false)
        .await
        .unwrap()
        .value
        .unwrap();
    assert!(
        matches!(recovered.page, CatalogPageView::Album { playback_revision, .. } if playback_revision == album.revision)
    );
    assert!(
        store
            .values("9", "catalog-album-playback-marker-v2")
            .unwrap()
            .is_empty()
    );
    let result = client
        .play_selection("1", selection(album, Some(row(vec![0, 1, 2], 2, "13"))))
        .await
        .unwrap();
    assert_eq!(result.track.unwrap().id, "13");
    assert_eq!(ids(&client).await, ["11", "13", "12"]);
    assert_eq!(
        store
            .values("9", "catalog-album-playback-marker-v2")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store.values("9", "catalog-album-pages-v3").unwrap().len(),
        1
    );
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|path| path.starts_with("/api/album/"))
    );
}

#[tokio::test]
async fn double_click_on_active_same_song_toggles_exact_decoder_until_it_is_inactive() {
    let (_dir, api, _session, client, _count) = fixture();
    *api.album_tracks.lock().unwrap() = Some(tracks(180_000));
    let (album, _) = scope(&client, AlbumDurationOrder::Ascending).await;
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: cumulus_core::playback::PlaybackMode::SingleLoop,
            },
        )
        .await
        .unwrap();
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["11".into(), "1000".into()],
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let before = client
        .player_action("1", PlayerAction::Seek { seconds: 17.0 })
        .await
        .unwrap();
    assert_eq!(before.state, PlaybackStatus::Playing);
    let select = |album| PlaySelection::SongRowDoubleClick {
        selection: SongRowSelection::AlbumDetail {
            album,
            row: row(vec![0, 1, 2], 0, "11"),
            quality: Quality::Standard,
        },
    };
    for replace_queue in [true, false] {
        if !replace_queue {
            client
                .queue_click_preferences_command(
                    "1",
                    QueueClickPreferencesCommand::Change {
                        expected_revision: "0".into(),
                        replace_queue,
                    },
                )
                .unwrap();
        }
        api.calls.lock().unwrap().clear();
        for state in [PlaybackStatus::Paused, PlaybackStatus::Playing] {
            let toggled = client
                .play_selection("1", select(album.clone()))
                .await
                .unwrap();
            assert_eq!(toggled.state, state);
            assert_eq!(toggled.entry_id, before.entry_id);
            assert_eq!(toggled.play_id, before.play_id);
            assert_eq!(toggled.queue_revision, before.queue_revision);
            assert_eq!(toggled.position, before.position);
            assert_eq!(ids(&client).await, ["11", "1000"]);
        }
        assert!(!api.calls.lock().unwrap().iter().any(|path| matches!(
            path.as_str(),
            "/api/v3/song/detail" | "/api/song/enhance/player/url/v1"
        )));
    }
    client
        .queue_click_preferences_command(
            "1",
            QueueClickPreferencesCommand::Change {
                expected_revision: "1".into(),
                replace_queue: true,
            },
        )
        .unwrap();

    // A valid duplicate ID must not revive a row whose album metadata was
    // omitted. Its new current receipt still carries the unavailable anchor.
    let mut missing = tracks(180_000);
    missing[0].as_object_mut().unwrap().remove("name");
    *api.album_tracks.lock().unwrap() = Some(missing);
    let (disabled, _) = scope(&client, AlbumDurationOrder::Ascending).await;
    assert!(matches!(
        client.play_selection("1", select(disabled)).await,
        Err(Error::QueueChanged)
    ));
    let unchanged = client.player_snapshot("1").await.unwrap();
    assert_eq!(unchanged.state, PlaybackStatus::Playing);
    assert_eq!(unchanged.play_id, before.play_id);
    assert_eq!(unchanged.queue_revision, before.queue_revision);

    *api.album_tracks.lock().unwrap() = Some(tracks(180_000));
    let (album, _) = scope(&client, AlbumDurationOrder::Ascending).await;
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    let replaced = client.play_selection("1", select(album)).await.unwrap();
    assert_eq!(replaced.state, PlaybackStatus::Playing);
    assert_ne!(replaced.play_id, before.play_id);
    assert_eq!(ids(&client).await, ["12", "11", "13"]);
}
