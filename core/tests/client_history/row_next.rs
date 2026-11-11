use super::*;
use cumulus_core::playback::PlaybackMode;

async fn row_next(f: &Fixture, kind: HistoryKind, clicked: &str, ids: &[&str]) -> PlaySelection {
    let page = f
        .client
        .history_page(&f.epoch(), kind, false)
        .await
        .unwrap();
    PlaySelection::HistoryRowNext {
        history_kind: kind,
        revision: page.value.revision,
        clicked_id: clicked.into(),
        selected_ids: ids.iter().map(|id| (*id).into()).collect(),
        quality: Quality::Standard,
    }
}

async fn ticket(f: &Fixture) -> String {
    let epoch = f.epoch();
    let player = f.client.player_snapshot(&epoch).await.unwrap();
    f.client
        .prepare_play_selection(&epoch, &player.queue_revision, player.play_id.as_deref())
        .unwrap()
}

async fn commit(f: &Fixture, selection: PlaySelection) -> cumulus_core::Result<PlayerView> {
    f.client
        .commit_prepared_selection(&f.epoch(), &ticket(f).await, selection)
        .await
}

#[tokio::test]
async fn row_next_splices_in_gesture_order_and_leaves_the_paused_decoder_untouched() {
    for mode in [PlaybackMode::Sequential, PlaybackMode::Random] {
        let f = fixture(true);
        let epoch = f.epoch();
        let kind = HistoryKind::Song;
        seed_history_batch(
            &f,
            kind,
            &[(1, 500), (2, 400), (3, 300), (4, 200), (5, 100)],
        );
        f.client
            .play_selection(
                &epoch,
                f.selection(kind, vec!["1".into(), "4".into(), "5".into()])
                    .await,
            )
            .await
            .unwrap();
        f.client
            .player_action(&epoch, PlayerAction::Mode { mode })
            .await
            .unwrap();
        f.client
            .player_action(&epoch, PlayerAction::Seek { seconds: 12.0 })
            .await
            .unwrap();
        let paused = f
            .client
            .player_action(&epoch, PlayerAction::Pause { paused: true })
            .await
            .unwrap();
        let loads = f.device.state.lock().unwrap().loads;
        let updated = commit(&f, row_next(&f, kind, "2", &["5", "3", "2"]).await)
            .await
            .unwrap();
        assert_eq!(history_batch_ids(&f).await, ["1", "5", "3", "2", "4"]);
        assert_ne!(updated.queue_revision, paused.queue_revision);
        assert_eq!(updated.play_id, paused.play_id);
        assert_eq!(updated.entry_id, paused.entry_id);
        assert_eq!(updated.position, paused.position);
        assert_eq!(updated.state, paused.state);
        assert_eq!(f.device.state.lock().unwrap().loads, loads);

        // Random mode must use the inserted first item as the next traversal
        // item, independently of where the renderer painted the clicked row.
        let next = f
            .client
            .player_action(&epoch, PlayerAction::Next)
            .await
            .unwrap();
        assert_eq!(next.track.unwrap().id, "5");
    }
}

#[tokio::test]
async fn row_next_on_an_empty_queue_starts_the_clicked_voice_in_random_mode() {
    let f = fixture(true);
    let epoch = f.epoch();
    let kind = HistoryKind::Voice;
    seed_history_batch(&f, kind, &[(5, 500), (6, 400), (7, 300)]);
    f.client
        .player_action(
            &epoch,
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    let started = commit(&f, row_next(&f, kind, "6", &["7", "5", "6"]).await)
        .await
        .unwrap();
    let track = started.track.unwrap();
    assert_eq!(track.voice_id.as_deref(), Some("6"));
    assert_eq!(track.id, "1006");
    assert_eq!(history_batch_ids(&f).await, ["7", "5", "6"]);
    assert_eq!(f.api.voice_rights.lock().unwrap().first(), Some(&6));
    assert_eq!(
        f.api.media_sources.lock().unwrap().first(),
        Some(&json!("1006"))
    );
    assert!(
        !f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|p| p == "/api/v6/dj/program/byradio")
    );
}

#[tokio::test]
async fn row_next_rejects_a_blocked_clicked_song_without_using_a_playable_sibling() {
    let f = fixture(true);
    let epoch = f.epoch();
    let kind = HistoryKind::Song;
    seed_history_batch(&f, kind, &[(1, 500), (2, 400), (3, 300)]);
    f.client
        .play_selection(&epoch, f.selection(kind, vec!["1".into()]).await)
        .await
        .unwrap();
    let paused = f
        .client
        .player_action(&epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    *f.api.blocked_songs.lock().unwrap() = vec!["2".into()];
    f.api.media_sources.lock().unwrap().clear();
    assert!(
        commit(&f, row_next(&f, kind, "2", &["3", "2"]).await)
            .await
            .is_err()
    );
    let current = f.client.player_snapshot(&epoch).await.unwrap();
    assert_eq!(current.queue_revision, paused.queue_revision);
    assert_eq!(current.play_id, paused.play_id);
    assert_eq!(current.state, paused.state);
    assert_eq!(history_batch_ids(&f).await, ["1"]);
    assert!(f.api.media_sources.lock().unwrap().is_empty());
}

#[tokio::test]
async fn row_next_follows_only_the_declared_same_recording_and_keeps_its_display_slot() {
    let f = fixture(true);
    let kind = HistoryKind::Song;
    seed_history_batch(&f, kind, &[(2, 500), (3, 400), (4, 300)]);
    *f.api.blocked_songs.lock().unwrap() = vec!["2".into()];
    *f.api.recording_links.lock().unwrap() = BTreeMap::from([("2".into(), "3".into())]);
    let started = commit(&f, row_next(&f, kind, "2", &["4", "3", "2"]).await)
        .await
        .unwrap();
    assert_eq!(started.track.unwrap().id, "3");
    assert_eq!(history_batch_ids(&f).await, ["4", "3"]);
    assert_eq!(
        *f.api.recording_reads.lock().unwrap(),
        [json!({"c":r#"[{"id":"3","v":0}]"#, "trialMode":-1})]
    );
    assert_eq!(
        f.api.media_sources.lock().unwrap().first(),
        Some(&json!("3"))
    );
}

#[tokio::test]
async fn row_next_cannot_escape_its_native_selection_or_reuse_stale_receipts() {
    let f = fixture(true);
    let epoch = f.epoch();
    let kind = HistoryKind::Song;
    seed_history_batch(&f, kind, &[(1, 500), (2, 400), (3, 300)]);
    let direct = row_next(&f, kind, "2", &[]).await;
    assert!(matches!(
        f.client.play_selection(&epoch, direct).await,
        Err(Error::InvalidInput(_))
    ));
    for (clicked, ids) in [
        ("99", &[][..]),
        ("2", &["2", "2"][..]),
        ("2", &["3", "99"][..]),
    ] {
        assert!(
            commit(&f, row_next(&f, kind, clicked, ids).await)
                .await
                .is_err()
        );
    }
    let retired = row_next(&f, kind, "2", &[]).await;
    seed_history_batch(&f, kind, &[(4, 600)]);
    assert!(commit(&f, retired).await.is_err());
    let stale = ticket(&f).await;
    let selection = row_next(&f, kind, "2", &[]).await;
    f.client
        .player_action(
            &epoch,
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    assert!(
        f.client
            .commit_prepared_selection(&epoch, &stale, selection)
            .await
            .is_err()
    );
    let stale = ticket(&f).await;
    let selection = row_next(&f, kind, "2", &[]).await;
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(
        f.client
            .commit_prepared_selection(&epoch, &stale, selection)
            .await
            .is_err()
    );
    assert!(history_batch_ids(&f).await.is_empty());
    assert!(f.api.calls.lock().unwrap().is_empty());
}
