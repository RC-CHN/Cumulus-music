use super::*;
use cumulus_core::{listen_together::TogetherService, playback::controller::PlaybackStatus};

fn end_count(api: &Backend) -> usize {
    api.calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(p, _)| p.ends_with("end/v2"))
        .count()
}
async fn clear(client: &MusicClient, queue: &QueueView) -> cumulus_core::Result<PlayerView> {
    // MutationBusy is returned at the host gate before any submission. Never
    // repeat a request that reached the actor or a remote room write.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    loop {
        let result = client
            .player_action(
                "1",
                PlayerAction::QueueFor {
                    expected_queue_revision: queue.revision.clone(),
                    expected_room_id: queue.clear_context.room_id.clone(),
                    edit: QueueEdit::Clear,
                },
            )
            .await;
        if matches!(result, Err(Error::MutationBusy)) && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        } else {
            return result;
        }
    }
}
async fn pause(client: &MusicClient) -> PlayerView {
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 12.0 })
        .await
        .unwrap()
}

#[tokio::test]
async fn room_identity_changes_invalidate_the_same_queue_without_clearing_or_ending_it() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (session, client) = world(root.path(), api.clone());
    play(&client).await;
    let before = pause(&client).await;
    let ordinary = client.player_queue("1").await.unwrap();
    assert!(ordinary.clear_context.room_id.is_none() && ordinary.clear_context.issue.is_none());
    api.state.lock().unwrap().inside = true;
    let service = TogetherService::new(session.clone()).unwrap();
    service
        .refresh_for(&session.token().unwrap(), 100)
        .await
        .unwrap();
    let observed = client.player_queue("1").await.unwrap();
    assert_eq!(observed.revision, ordinary.revision);
    assert_eq!(observed.clear_context.room_id.as_deref(), Some("test-room"));
    assert!(observed.clear_context.issue.unwrap().contains("尚未连接"));
    assert!(matches!(
        clear(&client, &ordinary).await,
        Err(Error::QueueChanged)
    ));
    assert!(matches!(
        clear(&client, &observed).await,
        Err(Error::State(
            "together queue requires recovery before clearing"
        ))
    ));
    api.state.lock().unwrap().replacement_room = Some("new-room".into());
    service
        .refresh_for(&session.token().unwrap(), 101)
        .await
        .unwrap();
    assert!(matches!(
        clear(&client, &observed).await,
        Err(Error::QueueChanged)
    ));
    let after = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_eq!(after.play_id, before.play_id);
    assert_eq!(after.position, Some(12.0));
    assert_eq!(after.queue_len, 3);
    assert_eq!(end_count(&api), 0);
    let projected = serde_json::to_string(&client.player_queue("1").await.unwrap()).unwrap();
    for private in ["private-channel", "test-account", "SECRET", "chatRoomId"] {
        assert!(!projected.contains(private));
    }
    session
        .adopt(api.clone(), json!({"userId":2}), &session.token().unwrap())
        .unwrap();
    let replacement = client.player_queue("2").await.unwrap();
    assert!(replacement.clear_context.room_id.is_none());
    assert!(matches!(
        client.player_queue("1").await,
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn confirmed_waiting_and_joined_clears_end_once_and_restore_local_preferences() {
    for peer in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let api = Arc::new(Backend::default());
        let (_, client) = world(root.path(), api.clone());
        let device = Arc::new(im::Device::default());
        let client = client.with_together_im_factory(Arc::new(im::Factory(device.clone())));
        play(&client).await;
        client
            .player_action("1", PlayerAction::Speed { value: 1.25 })
            .await
            .unwrap();
        client
            .player_action(
                "1",
                PlayerAction::Mode {
                    mode: PlaybackMode::Sequential,
                },
            )
            .await
            .unwrap();
        if peer {
            incoming_tests::joined(&client, &api, &device).await;
        } else {
            client
                .together_command("1", TogetherCommand::Create)
                .await
                .unwrap();
            wait(&client, |v| v.target_id.is_some() && v.pending.is_empty()).await;
        }
        pause(&client).await;
        wait(&client, |v| {
            v.progress_ms == Some(12000) && !v.playing && v.pending.is_empty()
        })
        .await;
        let queue = client.player_queue("1").await.unwrap();
        assert_eq!(queue.clear_context.room_id.as_deref(), Some("test-room"));
        assert!(queue.clear_context.issue.is_none());
        let cleared = clear(&client, &queue).await.unwrap();
        assert_eq!(cleared.state, PlaybackStatus::Idle);
        assert_eq!(cleared.queue_len, 0);
        wait(&client, |v| {
            v.room.is_none() && !v.attached && !v.local_recovery && v.lifecycle_notice.is_some()
        })
        .await;
        let after = client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap();
        assert_eq!(after.speed, 1.25);
        assert_eq!(after.mode, PlaybackMode::Sequential);
        assert!(after.play_id.is_none());
        assert_eq!(end_count(&api), 1);
        assert_eq!(device.closed.load(Ordering::SeqCst), 1);
        client.close_together().await.unwrap();
        assert_eq!(end_count(&api), 1);
    }
}

#[tokio::test]
async fn unknown_end_after_clear_is_only_reconciled_and_never_replayed() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    play(&client).await;
    client
        .together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    wait(&client, |v| v.target_id.is_some() && v.pending.is_empty()).await;
    pause(&client).await;
    wait(&client, |v| {
        v.progress_ms == Some(12000) && v.pending.is_empty()
    })
    .await;
    let queue = client.player_queue("1").await.unwrap();
    api.drop_end.store(true, Ordering::SeqCst);
    clear(&client, &queue).await.unwrap();
    let pending = wait(&client, |v| !v.pending.is_empty() && v.sync_error.is_some()).await;
    assert_eq!(pending.pending[0].action, "end");
    assert_eq!(pending.tracks.len(), 3);
    assert!(
        pending
            .tracks
            .iter()
            .all(|track| track.name.starts_with("Song") && track.duration_ms == 120000)
    );
    assert_eq!(end_count(&api), 1);
    let empty = client.player_queue("1").await.unwrap();
    assert!(empty.tracks.is_empty() && empty.clear_context.issue.is_some());
    client
        .together_command(
            "1",
            TogetherCommand::Reconcile {
                operation_id: pending.pending[0].id.clone(),
            },
        )
        .await
        .unwrap();
    wait(&client, |v| {
        v.room.is_none() && !v.attached && !v.local_recovery
    })
    .await;
    assert!(
        client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap()
            .play_id
            .is_none()
    );
    assert_eq!(end_count(&api), 1);
}

#[tokio::test]
async fn failed_local_clear_preserves_the_room_and_the_paused_audio() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    play(&client).await;
    client
        .together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    wait(&client, |v| v.target_id.is_some() && v.pending.is_empty()).await;
    let before = pause(&client).await;
    wait(&client, |v| {
        v.progress_ms == Some(12000) && v.pending.is_empty()
    })
    .await;
    let queue = client.player_queue("1").await.unwrap();
    let sql = rusqlite::Connection::open(root.path().join("core.sqlite3")).unwrap();
    sql.execute_batch("CREATE TRIGGER reject_queue_clear BEFORE INSERT ON state WHEN NEW.kind='playback' AND NEW.key='queue' BEGIN SELECT RAISE(ABORT, 'synthetic queue failure'); END;").unwrap();
    assert!(matches!(
        clear(&client, &queue).await,
        Err(Error::Storage(_))
    ));
    let after = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_eq!(after.play_id, before.play_id);
    assert_eq!(after.position, Some(12.0));
    assert_eq!(after.state, PlaybackStatus::Paused);
    assert_eq!(after.queue_len, 3);
    assert!(api.state.lock().unwrap().inside);
    assert_eq!(end_count(&api), 0);
    sql.execute_batch("DROP TRIGGER reject_queue_clear;")
        .unwrap();
    client.close_together().await.unwrap();
}
