use super::*;

fn reference(row: &LocalTrackView) -> LocalTrackReference {
    LocalTrackReference {
        id: row.track.id.clone(),
        revision: row.revision.clone(),
    }
}

#[tokio::test]
async fn local_batch_is_atomic_across_pages_and_keeps_the_loaded_queue() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let names = ["Fixture Zulu", "Fixture Middle", "Fixture Alpha", "Outside"];
    for (index, name) in names.iter().enumerate() {
        std::fs::write(media.join(format!("{name}.wav")), [index as u8]).unwrap();
    }
    let state = root.path().join("state");
    let (_, music, library) = client(&state, Arc::new(Probe::default()));
    library.scan(&media).unwrap();
    let query = LocalQuery {
        keyword: "fixture".into(),
        limit: 1,
        ..Default::default()
    };
    let first = music.local_page(query.clone()).await.unwrap();
    let last = music
        .local_page(LocalQuery {
            offset: 2,
            revision: Some(first.revision.clone()),
            ..query.clone()
        })
        .await
        .unwrap();
    assert_eq!((first.total, last.offset), (3, 2));
    assert_eq!(first.tracks[0].track.name, "Fixture Zulu");
    assert_eq!(last.tracks[0].track.name, "Fixture Alpha");
    assert_eq!(first.revision, last.revision);
    let outside = music
        .local_page(LocalQuery {
            keyword: "outside".into(),
            ..query.clone()
        })
        .await
        .unwrap();
    let first_row = &first.tracks[0];
    let last_row = &last.tracks[0];
    let command = |tracks| LocalCommand::HideTracks {
        query: query.clone(),
        revision: first.revision.clone(),
        tracks,
    };
    let loaded = music
        .play_selection(
            "0",
            PlaySelection::Local {
                query: query.clone(),
                revision: first.revision.clone(),
                start_id: Some(first_row.track.id.clone()),
            },
        )
        .await
        .unwrap();
    let paused = music
        .player_action("0", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    assert_eq!(loaded.play_id, paused.play_id);
    assert_eq!(paused.queue_len, 3);

    for invalid in [
        vec![reference(first_row), reference(&outside.tracks[0])],
        vec![reference(first_row), reference(first_row)],
        vec![
            reference(first_row),
            LocalTrackReference {
                revision: "0".repeat(64),
                ..reference(last_row)
            },
        ],
    ] {
        assert!(matches!(
            music.local_command(command(invalid)).await,
            Err(Error::QueueChanged)
        ));
        let unchanged = music.local_page(query.clone()).await.unwrap();
        assert_eq!(
            (unchanged.total, unchanged.revision),
            (3, first.revision.clone())
        );
    }

    music
        .local_command(command(vec![reference(first_row), reference(last_row)]))
        .await
        .unwrap();
    let remaining = music.local_page(query.clone()).await.unwrap();
    assert_eq!(
        (remaining.total, remaining.tracks[0].track.name.as_str()),
        (1, "Fixture Middle")
    );
    assert_ne!(remaining.revision, first.revision);
    assert!(matches!(
        music
            .local_command(command(vec![reference(first_row), reference(last_row)]))
            .await,
        Err(Error::QueueChanged)
    ));
    let still = music.player_snapshot("0").await.unwrap();
    assert_eq!(still.play_id, paused.play_id);
    assert_eq!(still.entry_id, paused.entry_id);
    assert_eq!(still.queue_revision, paused.queue_revision);
    assert_eq!(still.position, paused.position);
    assert_eq!(still.state, paused.state);
    for (index, name) in names.iter().enumerate() {
        assert_eq!(
            std::fs::read(media.join(format!("{name}.wav"))).unwrap(),
            [index as u8]
        );
    }

    library.scan(&media).unwrap();
    assert_eq!(music.local_page(query.clone()).await.unwrap().total, 1);
    drop(music);
    let (_, reopened, _) = client(&state, Arc::new(Probe::default()));
    assert_eq!(reopened.local_page(query.clone()).await.unwrap().total, 1);
    reopened.local_command(LocalCommand::Rescan).await.unwrap();
    assert_eq!(reopened.local_page(query).await.unwrap().total, 3);
}
