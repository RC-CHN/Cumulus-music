use super::incoming_tests::{command, joined, send};
use super::*;
use cumulus_core::{
    listen_together::TogetherService, playback::PlaybackQueue, storage::StateStore,
};

fn endings(api: &Backend) -> usize {
    api.calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(p, _)| p.ends_with("end/v2"))
        .count()
}

#[tokio::test]
async fn ending_an_observed_unattached_room_does_not_initialize_an_audio_device() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    api.state.lock().unwrap().inside = true;
    let (_, client) = world(root.path(), api.clone());
    let client = client.with_audio_factory(|| Err(Error::State("audio device must stay unopened")));
    client.together_view("1", true).await.unwrap();
    let result = client
        .together_command(
            "1",
            TogetherCommand::End {
                room_id: "test-room".into(),
            },
        )
        .await
        .unwrap();
    assert!(result.value.room.is_none() && !result.value.local_recovery);
    assert_eq!(endings(&api), 1);
}
async fn preferences(client: &MusicClient) {
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
}
async fn restored(client: &MusicClient) {
    wait(client, |v| {
        !v.attached && !v.local_recovery && v.lifecycle_notice.is_some()
    })
    .await;
    assert!(!client.has_together_cleanup().unwrap());
    let snapshot = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_eq!(snapshot.speed, 1.25);
    assert_eq!(snapshot.mode, PlaybackMode::Sequential);
}
async fn preview(client: &MusicClient, api: &Backend, im: &im::Device) {
    api.state.lock().unwrap().play = Some(command(100, "PAUSE", "91", 44000, "PAUSE"));
    *api.common.lock().unwrap()=Some(["91","92","93"].map(|id| json!({"songId":id,"playable":id!="91","unplayableType":if id=="91" {"trial"} else {""},"unplayableUserIds":[2],"unplayableOuterIds":[],"start":40,"end":50})).to_vec());
    send(im, 20012, json!({"roomId":"test-room"}));
    wait(client, |v| {
        v.common_access.iter().any(|p| p.preview.is_some())
    })
    .await;
}

#[tokio::test]
async fn peer_end_restores_preview_and_preferences_and_waits_for_sdk_cleanup_once() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    preferences(&client).await;
    joined(&client, &api, &im).await;
    preview(&client, &api, &im).await;
    im.hold_close.store(true, Ordering::SeqCst);
    send(&im, 20003, json!({"roomId":"test-room"}));
    send(&im, 20003, json!({"roomId":"test-room"}));
    wait(&client, |v| v.ending).await;
    assert!(client.has_together_cleanup().unwrap());
    assert_eq!(im.closed.load(Ordering::SeqCst), 0);
    im.hold_close.store(false, Ordering::SeqCst);
    restored(&client).await;
    assert_eq!(endings(&api), 1);
    assert_eq!(im.closed.load(Ordering::SeqCst), 1);
    let snapshot = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_eq!(snapshot.position, Some(44.0));
    assert_eq!(
        snapshot.state,
        cumulus_core::playback::controller::PlaybackStatus::Paused
    );
    let queue = PlaybackQueue::new(Some(Arc::new(StateStore::open(root.path()).unwrap())), "1")
        .unwrap()
        .snapshot();
    assert!(
        queue
            .items
            .iter()
            .all(|e| e.resource.get("shared_room").is_none()
                && e.resource.get("playback_constraint").is_none())
    );
    client.close_together().await.unwrap();
    assert_eq!(endings(&api), 1);
}

#[tokio::test]
async fn authoritative_room_loss_or_replacement_restores_only_the_bound_room_without_end_writes() {
    for replacement in [None, Some("another-room".to_string())] {
        let root = tempfile::tempdir().unwrap();
        let api = Arc::new(Backend::default());
        let (_, client) = world(root.path(), api.clone());
        let im = Arc::new(im::Device::default());
        let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
        play(&client).await;
        preferences(&client).await;
        joined(&client, &api, &im).await;
        {
            let mut state = api.state.lock().unwrap();
            state.inside = replacement.is_some();
            state.replacement_room = replacement.clone();
        }
        client.together_view("1", true).await.unwrap();
        restored(&client).await;
        assert_eq!(endings(&api), 0);
        let view = client.together_view("1", false).await.unwrap().value;
        assert_eq!(view.room.map(|r| r.id), replacement);
        client.close_together().await.unwrap();
        assert_eq!(endings(&api), 0);
    }
}

#[tokio::test]
async fn failed_automatic_sdk_cleanup_keeps_the_binding_and_never_claims_recovery() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    joined(&client, &api, &im).await;
    im.close_failure.store(true, Ordering::SeqCst);
    send(&im, 20003, json!({"roomId":"test-room"}));
    let view = wait(&client, |v| {
        v.room.is_none() && !v.attached && v.sync_error.is_some()
    })
    .await;
    assert!(view.local_recovery && view.lifecycle_notice.is_none());
    assert_eq!(
        view.connection.state,
        cumulus_core::listen_together::transport::ConnectionState::Failed
    );
    assert!(client.close_together().await.is_err());
    assert!(
        client
            .together_command("1", TogetherCommand::RestoreLocal)
            .await
            .is_err()
    );
    assert!(client.has_together_cleanup().unwrap());
    assert_eq!(endings(&api), 1);
}

#[tokio::test]
async fn lost_automatic_end_acknowledgement_blocks_reports_and_needs_read_only_reconciliation() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    preferences(&client).await;
    joined(&client, &api, &im).await;
    preview(&client, &api, &im).await;
    api.drop_end.store(true, Ordering::SeqCst);
    send(&im, 20003, json!({"roomId":"test-room"}));
    let pending = wait(&client, |v| {
        v.ending && v.pending.len() == 1 && v.sync_error.is_some()
    })
    .await;
    let before = api.calls.lock().unwrap().len();
    assert_eq!(im.closed.load(Ordering::SeqCst), 1);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(api.calls.lock().unwrap().len(), before);
    client
        .together_command(
            "1",
            TogetherCommand::Reconcile {
                operation_id: pending.pending[0].id.clone(),
            },
        )
        .await
        .unwrap();
    restored(&client).await;
    assert_eq!(
        client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap()
            .state,
        cumulus_core::playback::controller::PlaybackStatus::Idle,
        "removing shared constraints must not reload audio stopped during recovery"
    );
    assert_eq!(endings(&api), 1);
}

#[tokio::test]
async fn automatic_room_loss_reconciles_an_unknown_play_by_reads_before_removing_the_binding() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    preferences(&client).await;
    joined(&client, &api, &im).await;
    api.drop_play.store(true, Ordering::SeqCst);
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    wait(&client, |v| v.pending.len() == 1 && v.sync_error.is_some()).await;
    let writes = super::incoming_tests::writes(&api);
    api.state.lock().unwrap().inside = false;
    client.together_view("1", true).await.unwrap();
    restored(&client).await;
    assert!(
        client
            .together_view("1", false)
            .await
            .unwrap()
            .value
            .pending
            .is_empty()
    );
    assert_eq!(super::incoming_tests::writes(&api), writes);
    assert_eq!(endings(&api), 0);
}

#[tokio::test]
async fn account_switch_during_sdk_cleanup_preserves_old_recovery_without_changing_new_account() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (session, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    preferences(&client).await;
    joined(&client, &api, &im).await;
    im.hold_close.store(true, Ordering::SeqCst);
    send(&im, 20003, json!({"roomId":"test-room"}));
    wait(&client, |v| v.ending).await;
    session
        .adopt(api.clone(), json!({"userId":2}), &session.token().unwrap())
        .unwrap();
    client
        .player_action("2", PlayerAction::Speed { value: 1.5 })
        .await
        .unwrap();
    im.hold_close.store(false, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(4), async {
        while im.closed.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let view = client.together_view("2", false).await.unwrap().value;
    assert!(view.room.is_none() && view.lifecycle_notice.is_none() && !view.local_recovery);
    assert_eq!(
        client
            .subscribe_playback("2")
            .await
            .unwrap()
            .current()
            .unwrap()
            .speed,
        1.5
    );
    let store = StateStore::open(root.path()).unwrap();
    assert!(
        store
            .get("1", "listen-together-player-v1", "current")
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .get("2", "listen-together-player-v1", "current")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        endings(&api),
        0,
        "old token cannot end a new account's room"
    );
}

#[tokio::test]
async fn waiting_timeout_and_empty_queue_exit_also_restore_local_settings() {
    for timeout in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let api = Arc::new(Backend::default());
        let (session, client) = world(root.path(), api.clone());
        play(&client).await;
        preferences(&client).await;
        client
            .together_command("1", TogetherCommand::Create)
            .await
            .unwrap();
        wait(&client, |v| v.target_id.is_some() && v.pending.is_empty()).await;
        if timeout {
            let service = TogetherService::new(session.clone()).unwrap();
            let token = session.token().unwrap();
            let at = service.state_for(&token).unwrap().waiting_until_ms.unwrap();
            assert!(service.tick_for(&token, at).await.unwrap());
        } else {
            let queue = client.player_queue("1").await.unwrap();
            client
                .player_action(
                    "1",
                    PlayerAction::Remove {
                        entry_ids: queue.tracks.into_iter().map(|t| t.entry_id).collect(),
                    },
                )
                .await
                .unwrap();
        }
        restored(&client).await;
        assert_eq!(endings(&api), 1);
    }
}
