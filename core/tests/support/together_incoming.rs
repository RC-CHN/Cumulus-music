use super::*;
use cumulus_core::{
    listen_together::{
        IncomingPhase, RoomMessage, TogetherService, native_im::ImEvent, transport::ConnectionState,
    },
    playback::controller::PlaybackStatus,
};

pub(super) fn send(device: &im::Device, kind: u64, content: Value) {
    let message = RoomMessage::from_attachment(
        &json!({"content":{"type":kind,"content":content}}).to_string(),
    )
    .unwrap()
    .unwrap();
    device
        .controls
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .send(ImEvent::Business {
            messages: vec![message],
        })
        .unwrap();
}
pub(super) fn command(seq: u64, kind: &str, target: &str, progress: u64, status: &str) -> Value {
    json!({"roomId":"test-room","commandType":kind,"progress":progress,
        "playStatus":status,"formerSongId":"91","targetSongId":target,
        "clientSeq":seq,"serverSeq":seq,"userId":2})
}
pub(super) fn writes(api: &Backend) -> usize {
    api.calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path.ends_with("command/report"))
        .count()
}
pub(super) async fn joined(client: &MusicClient, api: &Backend, im: &im::Device) {
    client
        .together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    wait(client, |v| {
        v.connection.state == ConnectionState::Connected
            && v.connection.received == 1
            && v.target_id.is_some()
            && v.pending.is_empty()
    })
    .await;
    api.state.lock().unwrap().peer = true;
    send(im, 20002, json!({"roomId":"test-room"}));
    wait(client, |v| {
        v.room.as_ref().is_some_and(|r| r.members.len() == 2)
            && v.shared_phase == Some(IncomingPhase::Applied)
    })
    .await;
}
async fn receipt(session: &Arc<AccountSession>, seq: u64, phase: IncomingPhase) {
    let service = TogetherService::new(session.clone()).unwrap();
    let token = session.token().unwrap();
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            if service
                .state_for(&token)
                .unwrap()
                .incoming
                .is_some_and(|v| v.command.server_seq == Some(seq) && v.phase == phase)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("incoming receipt did not settle");
}

#[tokio::test]
async fn member_join_applies_shared_queue_even_when_http_observed_the_new_role_first() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (session, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 12.0 })
        .await
        .unwrap();
    client
        .together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    wait(&client, |v| {
        v.connection.state == ConnectionState::Connected
            && v.connection.received == 1
            && v.target_id.is_some()
            && v.pending.is_empty()
    })
    .await;
    api.state.lock().unwrap().peer = true;
    let service = TogetherService::new(session.clone()).unwrap();
    let observed = service
        .refresh_for(&session.token().unwrap(), 100)
        .await
        .unwrap();
    assert_ne!(
        observed.role,
        Some(cumulus_core::listen_together::RoomRole::Waiting)
    );
    assert!(observed.shared.is_none());
    let before = writes(&api);
    send(&im, 20002, json!({"roomId":"test-room"}));
    wait(&client, |v| {
        v.shared_phase == Some(IncomingPhase::Applied)
            && v.room.as_ref().is_some_and(|r| r.members.len() == 2)
    })
    .await;
    let native = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_eq!(native.state, PlaybackStatus::Paused);
    assert_eq!(native.position, Some(12.0));
    assert_eq!(
        writes(&api),
        before,
        "incoming membership read must not echo playlist/play writes"
    );
    client.close_together().await.unwrap();
}

#[tokio::test]
async fn decoded_business_commands_drive_native_player_without_echo_or_duplicate_rewind() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (session, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let audio = Arc::new(engine::Device::default());
    let output = audio.clone();
    let client = client
        .with_audio_factory(move || Ok(engine::audio_engine(output.clone())))
        .with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    joined(&client, &api, &im).await;
    let before = writes(&api);
    audio.state.lock().unwrap().pauses.clear();
    let first = command(100, "GOTO", "92", 33000, "PAUSE");
    api.state.lock().unwrap().play = Some(first.clone());
    send(&im, 20000, first.clone());
    receipt(&session, 100, IncomingPhase::Applied).await;
    let view = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_eq!(view.state, PlaybackStatus::Paused);
    assert_eq!(view.position, Some(33.0));
    assert_eq!(view.track.unwrap().id, "92");
    assert_eq!(audio.state.lock().unwrap().pauses, vec![true]);
    let second = command(101, "PROGRESS", "92", 41000, "PLAY");
    api.state.lock().unwrap().play = Some(second.clone());
    send(&im, 20000, second);
    receipt(&session, 101, IncomingPhase::Applied).await;
    send(&im, 20000, first);
    tokio::time::sleep(Duration::from_millis(250)).await;
    let view = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_eq!(view.position, Some(41.0));
    assert_eq!(
        view.state,
        PlaybackStatus::Paused,
        "same-track PROGRESS must not unpause based only on playStatus"
    );
    assert_eq!(
        writes(&api),
        before,
        "incoming player changes must not echo HTTP reports"
    );
    client.close_together().await.unwrap();
}

#[tokio::test]
async fn failed_remote_apply_requires_explicit_recovery_and_local_seek_does_not_retry_it() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (session, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let audio = Arc::new(engine::Device::default());
    let output = audio.clone();
    let client = client
        .with_audio_factory(move || Ok(engine::audio_engine(output.clone())))
        .with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    joined(&client, &api, &im).await;
    let before = writes(&api);
    audio.fail_seek.store(true, Ordering::SeqCst);
    let value = command(100, "PROGRESS", "91", 45000, "PLAY");
    api.state.lock().unwrap().play = Some(value.clone());
    send(&im, 20000, value);
    receipt(&session, 100, IncomingPhase::Failed).await;
    assert_eq!(
        client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap()
            .state,
        PlaybackStatus::Error
    );
    wait(&client, |v| v.sync_error.is_some()).await;
    audio.fail_seek.store(false, Ordering::SeqCst);
    client.player_action("1", PlayerAction::Play).await.unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 12.0 })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    receipt(&session, 100, IncomingPhase::Failed).await;
    assert_eq!(
        client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap()
            .position,
        Some(12.0)
    );
    assert_eq!(writes(&api), before);
    client
        .together_command(
            "1",
            TogetherCommand::Publish {
                room_id: "test-room".into(),
            },
        )
        .await
        .unwrap();
    receipt(&session, 100, IncomingPhase::Applied).await;
    assert_eq!(
        client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap()
            .position,
        Some(45.0)
    );
    assert_eq!(
        writes(&api),
        before,
        "recovery reads the room but does not send a new play command"
    );
    client.close_together().await.unwrap();
}

#[tokio::test]
async fn persisted_incoming_failure_can_recover_after_explicit_reattach() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (session, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let audio = Arc::new(engine::Device::default());
    let output = audio.clone();
    let client = client
        .with_audio_factory(move || Ok(engine::audio_engine(output.clone())))
        .with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    joined(&client, &api, &im).await;
    audio.fail_seek.store(true, Ordering::SeqCst);
    let value = command(100, "PAUSE", "91", 24000, "PAUSE");
    api.state.lock().unwrap().play = Some(value.clone());
    send(&im, 20000, value);
    receipt(&session, 100, IncomingPhase::Failed).await;
    drop(client);
    tokio::time::timeout(Duration::from_secs(3), async {
        while im.active.load(Ordering::SeqCst) != 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let before = writes(&api);
    let (session, reopened) = world(root.path(), api.clone());
    let reopened = reopened.with_together_im_factory(Arc::new(im::Factory(im.clone())));
    let view = reopened.together_view("1", false).await.unwrap().value;
    assert!(!view.attached && view.sync_error.is_some());
    reopened
        .together_command(
            "1",
            TogetherCommand::Attach {
                room_id: "test-room".into(),
            },
        )
        .await
        .unwrap();
    receipt(&session, 100, IncomingPhase::Applied).await;
    let view = reopened
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_eq!(view.position, Some(24.0));
    assert_eq!(view.state, PlaybackStatus::Paused);
    assert_eq!(writes(&api), before);
    reopened.close_together().await.unwrap();
}
