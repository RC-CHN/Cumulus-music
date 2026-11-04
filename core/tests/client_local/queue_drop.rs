use super::*;

#[tokio::test]
async fn local_drop_inserts_before_the_displayed_row_and_preserves_the_native_transport() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("media");
    std::fs::create_dir(&media).unwrap();
    for name in ["Set A", "Set B", "Extra C"] {
        std::fs::write(
            media.join(format!("{name}.wav")),
            b"synthetic audio engine fixture",
        )
        .unwrap();
    }
    let (_, client, library) = client(&root.path().join("state"), Arc::new(Probe::default()));
    library.scan(&media).unwrap();
    let device = Arc::new(support::Device::default());
    let client = client.with_audio_factory({
        let device = device.clone();
        move || Ok(support::audio_engine(device.clone()))
    });
    let queue_filter = LocalQuery {
        keyword: "Set".into(),
        descending: false,
        ..Default::default()
    };
    let initial = client.local_page(queue_filter.clone()).await.unwrap();
    assert_eq!(initial.tracks.len(), 2);
    let active = initial.tracks[0].track.id.clone();
    client
        .play_selection(
            "0",
            PlaySelection::Local {
                query: queue_filter.clone(),
                revision: initial.revision.clone(),
                start_id: Some(active.clone()),
            },
        )
        .await
        .unwrap();
    client
        .player_action("0", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let player = client
        .player_action("0", PlayerAction::Seek { seconds: 8.0 })
        .await
        .unwrap();
    let native_entry = device.state.lock().unwrap().view.entry_id;
    let original = client.player_queue("0").await.unwrap();
    let extra_filter = LocalQuery {
        keyword: "Extra".into(),
        ..Default::default()
    };
    let extra = client.local_page(extra_filter.clone()).await.unwrap();
    assert_eq!(extra.tracks.len(), 1);
    let extra_id = extra.tracks[0].track.id.clone();
    let attempt = |queue_revision: String, track_id: String, anchor: Option<String>| {
        PlayerAction::DropLocalIntoQueue {
            query: extra_filter.clone(),
            revision: extra.revision.clone(),
            track_id,
            expected_queue_revision: queue_revision,
            expected_play_id: player.play_id.clone(),
            before_entry_id: anchor,
        }
    };
    let inserted = client
        .player_action(
            "0",
            attempt(
                original.revision.clone(),
                extra_id.clone(),
                Some(original.tracks[0].entry_id.clone()),
            ),
        )
        .await
        .unwrap();
    assert_eq!(inserted.play_id, player.play_id);
    assert_eq!(inserted.entry_id, player.entry_id);
    assert_eq!(inserted.position, Some(8.0));
    assert_eq!(
        inserted.state,
        cumulus_core::playback::controller::PlaybackStatus::Paused
    );
    assert_eq!(device.state.lock().unwrap().view.entry_id, native_entry);
    let queue = client.player_queue("0").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|row| row.track.name.as_str())
            .collect::<Vec<_>>(),
        ["Extra C", "Set A", "Set B"]
    );

    assert!(matches!(
        client
            .player_action("0", attempt(original.revision, extra_id.clone(), None))
            .await,
        Err(Error::QueueChanged)
    ));
    assert!(
        client
            .player_action("0", attempt(queue.revision.clone(), active.clone(), None))
            .await
            .is_err(),
        "a different local query must not admit the active ID"
    );
    assert!(
        client
            .player_action(
                "0",
                attempt(
                    queue.revision.clone(),
                    extra_id,
                    Some("missing boundary".into())
                )
            )
            .await
            .is_err()
    );
    assert!(matches!(
        client
            .player_action(
                "0",
                PlayerAction::DropLocalIntoQueue {
                    query: LocalQuery {
                        limit: 1,
                        ..queue_filter.clone()
                    },
                    revision: initial.revision.clone(),
                    track_id: initial.tracks[1].track.id.clone(),
                    expected_queue_revision: queue.revision.clone(),
                    expected_play_id: player.play_id.clone(),
                    before_entry_id: None,
                }
            )
            .await,
        Err(Error::InvalidInput(
            "local drop row is outside the displayed page"
        ))
    ));
    assert_eq!(
        client.player_queue("0").await.unwrap().revision,
        queue.revision
    );

    let moved = client
        .player_action(
            "0",
            PlayerAction::DropLocalIntoQueue {
                query: queue_filter,
                revision: initial.revision,
                track_id: active,
                expected_queue_revision: queue.revision,
                expected_play_id: player.play_id.clone(),
                before_entry_id: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(moved.play_id, player.play_id);
    assert_eq!(moved.entry_id, player.entry_id);
    assert_eq!(moved.position, Some(8.0));
    assert_eq!(device.state.lock().unwrap().view.entry_id, native_entry);
    let queue = client.player_queue("0").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|row| row.track.name.as_str())
            .collect::<Vec<_>>(),
        ["Extra C", "Set B", "Set A"]
    );
    assert_eq!(queue.tracks[2].entry_id, original.tracks[0].entry_id);
}
