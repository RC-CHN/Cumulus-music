use super::{incoming_tests::joined, *};
use cumulus_core::listen_together::{TogetherPhase, TogetherService, transport::ConnectionState};

fn end_count(api: &Backend) -> usize {
    api.calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path.ends_with("end/v2"))
        .count()
}

async fn trigger(client: &MusicClient, api: &Backend, code: u16) {
    api.queue_rejection.store(code, Ordering::SeqCst);
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::SingleLoop,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn paired_queue_loss_closes_im_before_single_end_and_read_only_recovery() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (session, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    client
        .player_action("1", PlayerAction::Speed { value: 1.25 })
        .await
        .unwrap();
    joined(&client, &api, &im).await;
    trigger(&client, &api, 488).await;
    let view = wait(&client, |v| v.queue_room_loss.is_some()).await;
    assert!(view.ending && view.pending.is_empty());
    assert_eq!(view.connection.state, ConnectionState::Connected);
    assert!(
        !serde_json::to_string(&view)
            .unwrap()
            .contains("never serialize")
    );
    let loss = view.queue_room_loss.unwrap();
    let svc = TogetherService::new(session.clone()).unwrap();
    let token = session.token().unwrap();
    let rejected = svc
        .operations_for(&token)
        .unwrap()
        .into_iter()
        .find(|op| op.id == loss.report_id)
        .unwrap();
    assert_eq!(rejected.phase, TogetherPhase::Rejected);
    assert!(!rejected.acknowledged);

    let confirm = || TogetherCommand::ConfirmQueueRoomLoss {
        room_id: loss.room_id.clone(),
        report_id: loss.report_id.clone(),
    };
    assert!(client.together_command("2", confirm()).await.is_err());
    assert!(
        client
            .together_command(
                "1",
                TogetherCommand::ConfirmQueueRoomLoss {
                    room_id: loss.room_id.clone(),
                    report_id: "stale-report".into(),
                }
            )
            .await
            .is_err()
    );
    assert!(
        client
            .together_command(
                "1",
                TogetherCommand::End {
                    room_id: loss.room_id.clone(),
                }
            )
            .await
            .is_err()
    );
    assert_eq!(end_count(&api), 0);
    assert_eq!(im.closed.load(Ordering::SeqCst), 0);

    im.hold_close.store(true, Ordering::SeqCst);
    api.drop_end.store(true, Ordering::SeqCst);
    let (reply, ()) = tokio::join!(client.together_command("1", confirm()), async {
        tokio::time::timeout(Duration::from_secs(4), async {
            while !im.close_started.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(im.closed.load(Ordering::SeqCst), 0);
        assert_eq!(end_count(&api), 0, "must wait for SDK cleanup");
        im.hold_close.store(false, Ordering::SeqCst);
    });
    assert!(reply.is_err(), "simulated loss of successful end response");
    assert_eq!(im.closed.load(Ordering::SeqCst), 1);
    assert_eq!(end_count(&api), 1);
    let awaiting = client.together_view("1", false).await.unwrap().value;
    assert!(awaiting.ending && awaiting.queue_room_loss.is_none());
    assert_eq!(awaiting.pending.len(), 1);
    assert_eq!(awaiting.pending[0].phase, TogetherPhase::Uncertain);
    assert!(client.together_command("1", confirm()).await.is_err());
    // Opening the recovery page first reads status, clearing the old room
    // from state but not the rejected report's link in the End journal.
    let refreshed = client.together_view("1", true).await.unwrap().value;
    assert!(refreshed.room.is_none() && refreshed.local_recovery);
    assert_eq!(refreshed.pending[0].id, awaiting.pending[0].id);
    let reconciled = client
        .together_command(
            "1",
            TogetherCommand::Reconcile {
                operation_id: awaiting.pending[0].id.clone(),
            },
        )
        .await
        .unwrap()
        .value;
    assert!(reconciled.room.is_none() && !reconciled.local_recovery && !reconciled.ending);
    assert!(reconciled.pending.is_empty());
    assert_eq!(end_count(&api), 1);
    assert_eq!(
        client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap()
            .speed,
        1.25
    );

    // The sealed API deliberately reuses the room ID on a fresh creation.
    // A replay of the already-confirmed old End journal must not detach it.
    client
        .together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    wait(&client, |v| {
        v.attached && v.connection.state == ConnectionState::Connected
    })
    .await;
    let still_attached = client
        .together_command(
            "1",
            TogetherCommand::Reconcile {
                operation_id: awaiting.pending[0].id.clone(),
            },
        )
        .await
        .unwrap()
        .value;
    assert!(still_attached.attached && still_attached.room.is_some());
    assert_eq!(end_count(&api), 1);
    assert_eq!(im.closed.load(Ordering::SeqCst), 1);
    client.close_together().await.unwrap();
}

#[tokio::test]
async fn stale_room_loss_cannot_end_a_newly_observed_room() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    joined(&client, &api, &im).await;
    trigger(&client, &api, 488).await;
    let loss = wait(&client, |v| v.queue_room_loss.is_some())
        .await
        .queue_room_loss
        .unwrap();
    api.state.lock().unwrap().replacement_room = Some("different-room".into());
    let result = client
        .together_command(
            "1",
            TogetherCommand::ConfirmQueueRoomLoss {
                room_id: loss.room_id,
                report_id: loss.report_id,
            },
        )
        .await
        .unwrap()
        .value;
    assert_eq!(result.room.unwrap().id, "different-room");
    assert!(!result.local_recovery && result.queue_room_loss.is_none());
    assert_eq!(im.closed.load(Ordering::SeqCst), 1);
    assert_eq!(end_count(&api), 0);
}

#[tokio::test]
async fn queue_488_in_waiting_and_paired_auth_errors_do_not_open_room_loss() {
    for (paired, code, deny_http) in [(false, 488, false), (true, 403, false), (true, 0, true)] {
        let root = tempfile::tempdir().unwrap();
        let api = Arc::new(Backend::default());
        let (_, client) = world(root.path(), api.clone());
        let im = Arc::new(im::Device::default());
        let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
        play(&client).await;
        if paired {
            joined(&client, &api, &im).await;
        } else {
            client
                .together_command("1", TogetherCommand::Create)
                .await
                .unwrap();
            wait(&client, |v| {
                v.connection.state == ConnectionState::Connected && v.target_id.is_some()
            })
            .await;
        }
        api.deny_queue_http.store(deny_http, Ordering::SeqCst);
        trigger(&client, &api, code).await;
        let view = wait(&client, |v| v.sync_error.is_some()).await;
        assert!(!view.ending && view.queue_room_loss.is_none());
        assert_eq!(end_count(&api), 0);
        assert_eq!(im.closed.load(Ordering::SeqCst), 0);
        client.close_together().await.unwrap();
    }
}
