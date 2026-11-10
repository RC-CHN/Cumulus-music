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
    let [keep_title, target_title, outside_title] =
        ["keep", "target", "outside"].map(|role| fixture_query_term(&media, role));
    for name in [&keep_title, &target_title, &outside_title] {
        std::fs::write(media.join(format!("{name}.wav")), b"first indexed revision").unwrap();
    }
    let (_, client, library) = client(&root.path().join("state"), Arc::new(Probe::default()));
    library.scan(&media).unwrap();
    let keep_query = LocalQuery {
        keyword: keep_title.clone(),
        ..Default::default()
    };
    let keep = client.local_page(keep_query.clone()).await.unwrap();
    let query = LocalQuery {
        keyword: target_title.clone(),
        ..Default::default()
    };
    let target = client.local_page(query.clone()).await.unwrap();
    assert_eq!(keep.total, 1);
    assert_eq!(target.total, 1);
    assert_eq!(keep.tracks[0].track.name, keep_title);
    assert_eq!(target.tracks[0].track.name, target_title);
    let selection = |tracks| PlaySelection::LocalBatch {
        query: query.clone(),
        revision: target.revision.clone(),
        tracks,
    };
    client
        .play_selection(
            "0",
            PlaySelection::Local {
                query: keep_query,
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
    for (case, invalid) in [
        (
            "out-of-query row",
            vec![reference(&target.tracks[0]), reference(&keep.tracks[0])],
        ),
        (
            "duplicate row",
            vec![reference(&target.tracks[0]), reference(&target.tracks[0])],
        ),
        (
            "stale row revision",
            vec![LocalTrackReference {
                id: target.tracks[0].track.id.clone(),
                revision: "0".repeat(64),
            }],
        ),
    ] {
        let result = client
            .commit_prepared_selection("0", &prepare(&client).await, selection(invalid))
            .await;
        assert!(
            matches!(result, Err(Error::QueueChanged)),
            "{case}: expected queue change rejection; error was {:?}",
            result.err()
        );
    }
    // A filesystem replacement is stale even before a scan changes the index.
    std::fs::write(
        media.join(format!("{target_title}.wav")),
        b"new on-disk bytes",
    )
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

async fn add(
    client: &MusicClient,
    query: LocalQuery,
    revision: &str,
    tracks: Vec<LocalTrackReference>,
) -> Result<PlayerView> {
    client
        .commit_prepared_selection(
            "0",
            &prepare(client).await,
            PlaySelection::LocalBatchAdd {
                query,
                revision: revision.into(),
                tracks,
            },
        )
        .await
}

#[tokio::test]
async fn add_moves_exact_active_duplicate_without_resuming_or_rebinding() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("media");
    std::fs::create_dir(&media).unwrap();
    for name in ["Zulu", "Middle", "Delta", "Alpha"] {
        std::fs::write(media.join(format!("{name}.wav")), b"plain local").unwrap();
    }
    let (_, client, library) = client(&root.path().join("state"), Arc::new(Probe::default()));
    library.scan(&media).unwrap();
    let descending = LocalQuery::default();
    let ascending = LocalQuery {
        descending: false,
        ..descending.clone()
    };
    let view = client.local_page(ascending.clone()).await.unwrap();
    assert_eq!(view.tracks[2].track.name, "Middle");
    client
        .play_selection(
            "0",
            PlaySelection::Local {
                query: descending,
                revision: view.revision.clone(),
                start_id: Some(view.tracks[2].track.id.clone()),
            },
        )
        .await
        .unwrap();
    client
        .player_action("0", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let paused = client
        .player_action("0", PlayerAction::Seek { seconds: 12.0 })
        .await
        .unwrap();
    let updated = add(
        &client,
        ascending,
        &view.revision,
        vec![reference(&view.tracks[2]), reference(&view.tracks[0])],
    )
    .await
    .unwrap();
    assert_eq!(updated.play_id, paused.play_id);
    assert_eq!(updated.entry_id, paused.entry_id);
    assert_eq!(updated.position, paused.position);
    assert_eq!(updated.state, PlaybackStatus::Paused);
    assert_ne!(updated.queue_revision, paused.queue_revision);
    let queue = client.player_queue("0").await.unwrap();
    assert_eq!(rows(&queue), ["Zulu", "Alpha", "Middle", "Delta"]);
    assert_eq!(queue.current, paused.entry_id);
}

#[tokio::test]
async fn nonempty_random_queue_without_current_adds_at_head_but_empty_add_starts() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("media");
    std::fs::create_dir(&media).unwrap();
    for name in ["Existing", "Inserted"] {
        std::fs::write(media.join(format!("{name}.wav")), b"plain local").unwrap();
    }
    let state = root.path().join("state");
    let (_, client, library) = client(&state, Arc::new(Probe::default()));
    library.scan(&media).unwrap();
    let query = LocalQuery::default();
    let view = client.local_page(query.clone()).await.unwrap();
    let store = ProfileRegistry::open(&state, ApiOrigin::Pc)
        .unwrap()
        .store();
    let existing = library.resource(&view.tracks[1].track.id).unwrap();
    store
        .put(
            "local-guest",
            "playback",
            "queue",
            &json!({
                "items":[{"entry_id":"kept","resource":existing,"source":{"kind":"local"}}],
                "order":["kept"],"current":null,"mode":"random"
            }),
        )
        .unwrap();
    let idle = add(
        &client,
        query.clone(),
        &view.revision,
        vec![reference(&view.tracks[0])],
    )
    .await
    .unwrap();
    assert_eq!(idle.state, PlaybackStatus::Idle);
    assert!(idle.play_id.is_none() && idle.entry_id.is_none());
    let queue = client.player_queue("0").await.unwrap();
    assert_eq!(rows(&queue), ["Inserted", "Existing"]);
    assert!(queue.current.is_none());
    let clear = client
        .player_action(
            "0",
            PlayerAction::QueueFor {
                expected_queue_revision: idle.queue_revision,
                expected_room_id: None,
                edit: QueueEdit::Clear,
            },
        )
        .await
        .unwrap();
    assert_eq!(clear.queue_len, 0);
    let playing = add(
        &client,
        query,
        &view.revision,
        vec![reference(&view.tracks[0])],
    )
    .await
    .unwrap();
    assert_eq!(playing.state, PlaybackStatus::Playing);
    assert!(playing.play_id.is_some());
    assert_eq!(playing.track.unwrap().name, "Inserted");
}

#[tokio::test]
async fn random_add_schedules_first_new_row_next_without_touching_current_decoder() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let target = fixture_query_term(&media, "target");
    let target_z = format!("{target} Z");
    let target_a = format!("{target} A");
    for name in ["Keep", target_z.as_str(), target_a.as_str(), "Other"] {
        std::fs::write(media.join(format!("{name}.wav")), b"plain local").unwrap();
    }
    let (_, client, library) = client(&root.path().join("state"), Arc::new(Probe::default()));
    library.scan(&media).unwrap();
    let view = client.local_page(LocalQuery::default()).await.unwrap();
    let keep = view
        .tracks
        .iter()
        .find(|row| row.track.name == "Keep")
        .unwrap();
    client
        .play_selection(
            "0",
            PlaySelection::Local {
                query: LocalQuery::default(),
                revision: view.revision.clone(),
                start_id: Some(keep.track.id.clone()),
            },
        )
        .await
        .unwrap();
    client
        .player_action(
            "0",
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    let before = client
        .player_action("0", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let query = LocalQuery {
        keyword: target,
        ..Default::default()
    };
    let targets = client.local_page(query.clone()).await.unwrap();
    assert_eq!(targets.total, 2);
    let after = add(
        &client,
        query,
        &targets.revision,
        targets.tracks.iter().rev().map(reference).collect(),
    )
    .await
    .unwrap();
    assert_eq!(
        (after.play_id, after.entry_id, after.position, after.state),
        (
            before.play_id,
            before.entry_id,
            before.position,
            PlaybackStatus::Paused
        )
    );
    let next = client.player_action("0", PlayerAction::Next).await.unwrap();
    assert_eq!(next.track.unwrap().name, target_z);
}

#[tokio::test]
async fn add_does_not_rebind_current_to_a_replaced_local_file_revision() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let file = media.join("Current.wav");
    std::fs::write(&file, b"first local revision").unwrap();
    let state = root.path().join("state");
    let (_, client, library) = client(&state, Arc::new(Probe::default()));
    library.scan(&media).unwrap();
    let query = LocalQuery::default();
    let old = client.local_page(query.clone()).await.unwrap();
    let old_resource = library.resource(&old.tracks[0].track.id).unwrap();
    std::fs::write(&file, b"different length in newly scanned local file").unwrap();
    library.scan(&media).unwrap();
    let fresh = client.local_page(query.clone()).await.unwrap();
    assert_eq!(old.tracks[0].track.id, fresh.tracks[0].track.id);
    assert_ne!(old.tracks[0].revision, fresh.tracks[0].revision);
    let store = ProfileRegistry::open(&state, ApiOrigin::Pc)
        .unwrap()
        .store();
    store
        .put(
            "local-guest",
            "playback",
            "queue",
            &json!({
                "items":[{"entry_id":"kept","resource":old_resource,"source":{"kind":"local"}}],
                "order":["kept"],"current":"kept"
            }),
        )
        .unwrap();
    let before = client.player_snapshot("0").await.unwrap();
    assert!(matches!(
        add(
            &client,
            query,
            &fresh.revision,
            vec![reference(&fresh.tracks[0])]
        )
        .await,
        Err(Error::State(
            "local batch would replace the current file revision"
        ))
    ));
    let after = client.player_snapshot("0").await.unwrap();
    assert_eq!(after.play_id, before.play_id);
    assert_eq!(after.queue_revision, before.queue_revision);
    assert_eq!(
        client.player_queue("0").await.unwrap().current.as_deref(),
        Some("kept")
    );
}
