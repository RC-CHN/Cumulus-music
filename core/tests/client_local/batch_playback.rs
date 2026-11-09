use super::*;
use cumulus_core::playback::{PlaybackMode, controller::PlaybackStatus};

fn reference(row: &LocalTrackView) -> LocalTrackReference {
    LocalTrackReference {
        id: row.track.id.clone(),
        revision: row.revision.clone(),
    }
}

async fn prepare(client: &MusicClient) -> String {
    let view = client.player_snapshot("0").await.unwrap();
    client
        .prepare_play_selection("0", &view.queue_revision, view.play_id.as_deref())
        .unwrap()
}

fn rows(queue: &QueueView) -> Vec<&str> {
    queue
        .tracks
        .iter()
        .map(|row| row.track.name.as_str())
        .collect()
}

#[tokio::test]
async fn local_batch_uses_full_index_order_moves_existing_rows_and_resumes_exact_current() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("media");
    std::fs::create_dir(&media).unwrap();
    for name in ["Zulu", "Middle", "Delta", "Alpha"] {
        std::fs::write(media.join(format!("{name}.wav")), b"plain local").unwrap();
    }
    let (_, client, library) = client(&root.path().join("state"), Arc::new(Probe::default()));
    library.scan(&media).unwrap();
    let query = LocalQuery {
        limit: 1,
        ..Default::default()
    };
    let top = client.local_page(query.clone()).await.unwrap();
    let bottom = client
        .local_page(LocalQuery {
            revision: Some(top.revision.clone()),
            offset: 3,
            ..query.clone()
        })
        .await
        .unwrap();
    let middle = client
        .local_page(LocalQuery {
            offset: 1,
            ..query.clone()
        })
        .await
        .unwrap();
    let starting = client
        .play_selection(
            "0",
            PlaySelection::Local {
                query: query.clone(),
                revision: top.revision.clone(),
                start_id: Some(middle.tracks[0].track.id.clone()),
            },
        )
        .await
        .unwrap();
    let initial = client.player_queue("0").await.unwrap();
    assert_eq!(rows(&initial), ["Zulu", "Middle", "Delta", "Alpha"]);
    let alpha_entry = &initial.tracks[3].entry_id;
    let zulu_entry = &initial.tracks[0].entry_id;

    let make_selection = || PlaySelection::LocalBatch {
        query: query.clone(),
        revision: top.revision.clone(),
        // IPC array intentionally reversed across pages.
        tracks: vec![reference(&bottom.tracks[0]), reference(&top.tracks[0])],
    };
    assert!(matches!(
        client.play_selection("0", make_selection()).await,
        Err(Error::InvalidInput(_))
    ));
    let first = client
        .commit_prepared_selection("0", &prepare(&client).await, make_selection())
        .await
        .unwrap();
    let reordered = client.player_queue("0").await.unwrap();
    assert_eq!(rows(&reordered), ["Middle", "Zulu", "Alpha", "Delta"]);
    assert_eq!(&reordered.tracks[1].entry_id, zulu_entry);
    assert_eq!(&reordered.tracks[2].entry_id, alpha_entry);
    assert_eq!(reordered.current.as_ref(), Some(zulu_entry));
    assert_ne!(first.play_id, starting.play_id);
    assert_eq!(first.state, PlaybackStatus::Playing);
    client
        .player_action("0", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let paused = client
        .player_action("0", PlayerAction::Seek { seconds: 13.0 })
        .await
        .unwrap();
    assert_eq!(paused.state, PlaybackStatus::Paused);
    let resumed = client
        .commit_prepared_selection("0", &prepare(&client).await, make_selection())
        .await
        .unwrap();
    assert_eq!(resumed.play_id, first.play_id);
    assert_eq!(resumed.entry_id, first.entry_id);
    assert_eq!(resumed.position, Some(13.0));
    assert_eq!(resumed.state, PlaybackStatus::Playing);
    assert_eq!(resumed.queue_revision, reordered.revision);
}

#[tokio::test]
async fn invalid_refs_index_replacement_and_new_control_preserve_guest_playback() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("media");
    std::fs::create_dir(&media).unwrap();
    for name in ["Keep", "Target", "Outside"] {
        std::fs::write(media.join(format!("{name}.wav")), b"first indexed revision").unwrap();
    }
    let (_, client, library) = client(&root.path().join("state"), Arc::new(Probe::default()));
    library.scan(&media).unwrap();
    let keep = client
        .local_page(LocalQuery {
            keyword: "keep".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let query = LocalQuery {
        keyword: "target".into(),
        ..Default::default()
    };
    let target = client.local_page(query.clone()).await.unwrap();
    let selection = |tracks| PlaySelection::LocalBatch {
        query: query.clone(),
        revision: target.revision.clone(),
        tracks,
    };
    client
        .play_selection(
            "0",
            PlaySelection::Local {
                query: LocalQuery {
                    keyword: "keep".into(),
                    ..Default::default()
                },
                revision: keep.revision.clone(),
                start_id: None,
            },
        )
        .await
        .unwrap();
    let paused = client
        .player_action("0", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    for invalid in [
        vec![reference(&target.tracks[0]), reference(&keep.tracks[0])],
        vec![reference(&target.tracks[0]), reference(&target.tracks[0])],
        vec![LocalTrackReference {
            id: target.tracks[0].track.id.clone(),
            revision: "0".repeat(64),
        }],
    ] {
        assert!(matches!(
            client
                .commit_prepared_selection("0", &prepare(&client).await, selection(invalid))
                .await,
            Err(Error::QueueChanged)
        ));
    }
    // A filesystem replacement is stale even before a scan changes the index.
    std::fs::write(media.join("Target.wav"), b"new on-disk bytes").unwrap();
    assert!(matches!(
        client
            .commit_prepared_selection(
                "0",
                &prepare(&client).await,
                selection(vec![reference(&target.tracks[0])])
            )
            .await,
        Err(Error::QueueChanged)
    ));
    // An unrelated index edit invalidates the full-list revision as well.
    client
        .local_command(LocalCommand::HideTrack {
            id: keep.tracks[0].track.id.clone(),
            revision: keep.tracks[0].revision.clone(),
        })
        .await
        .unwrap();
    assert!(matches!(
        client
            .commit_prepared_selection(
                "0",
                &prepare(&client).await,
                selection(vec![reference(&target.tracks[0])])
            )
            .await,
        Err(Error::QueueChanged)
    ));
    let state = client.player_snapshot("0").await.unwrap();
    assert_eq!(
        (
            state.play_id,
            state.entry_id,
            state.queue_revision,
            state.position,
            state.state
        ),
        (
            paused.play_id,
            paused.entry_id,
            paused.queue_revision,
            paused.position,
            PlaybackStatus::Paused
        )
    );
    let ticket = prepare(&client).await;
    client
        .player_action("0", PlayerAction::Pause { paused: false })
        .await
        .unwrap();
    assert!(matches!(
        client
            .commit_prepared_selection("0", &ticket, selection(vec![reference(&target.tracks[0])]))
            .await,
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn batch_encrypted_rows_need_entitlement_and_random_selects_only_inserted_plain_rows() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("media");
    std::fs::create_dir(&media).unwrap();
    for name in ["Zulu.ncm", "B.wav", "A.wav"] {
        std::fs::write(media.join(name), b"sealed scan fixture").unwrap();
    }
    let (_, client, library) = client(&root.path().join("state"), Arc::new(Probe::default()));
    library.scan(&media).unwrap();
    let query = LocalQuery::default();
    let view = client.local_page(query.clone()).await.unwrap();
    assert_eq!(view.tracks[0].file_name, "Zulu.ncm");
    let selection = |tracks| PlaySelection::LocalBatch {
        query: query.clone(),
        revision: view.revision.clone(),
        tracks,
    };
    assert!(matches!(
        client
            .commit_prepared_selection(
                "0",
                &prepare(&client).await,
                selection(vec![reference(&view.tracks[0])])
            )
            .await,
        Err(Error::State("local batch has no playable plain audio"))
    ));
    assert_eq!(
        client.player_snapshot("0").await.unwrap().state,
        PlaybackStatus::Idle
    );
    client
        .player_action(
            "0",
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    let random = client
        .commit_prepared_selection(
            "0",
            &prepare(&client).await,
            selection(view.tracks.iter().rev().map(reference).collect()),
        )
        .await
        .unwrap();
    assert_eq!(random.state, PlaybackStatus::Playing);
    assert!(["B", "A"].contains(&random.track.as_ref().unwrap().name.as_str()));
    let queue = client.player_queue("0").await.unwrap();
    assert_eq!(rows(&queue), ["B", "A"]);
}
