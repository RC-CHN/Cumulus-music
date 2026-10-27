mod support;
use cumulus_core::{Error, account::AccountSession, listen_together::*, storage::StateStore};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use support::{Reply, ScriptApi, gate};
fn room() -> Value {
    json!({"roomId":"isolated-room","creatorId":1,"chatRoomId":"31","agoraChannelId":"41","roomUsers":[{"userId":1,"nickname":"one"}]})
}
fn status(inside: bool) -> Reply {
    json!({"code":200,"data":{"inRoom":inside,"roomInfo":if inside {room()} else {Value::Null}}})
        .into()
}
fn ack() -> Reply {
    json!({"code":200,"data":{"result":true}}).into()
}
fn snapshot(command: Option<PlayCommand>) -> Reply {
    let mut command = command;
    if let Some(c) = &mut command {
        c.user_id = Some("1".into());
        c.server_seq = Some(2);
    }
    json!({"code":200,"data":{"playlist":{"displayList":{"changed":true,"result":[91]},"randomList":null,"playMode":"ORDER_LOOP","version":[{"userId":1,"version":1}],"replace":true},"playCommand":command}}).into()
}
fn list() -> ListCommand {
    ListCommand {
        command_type: ListCommandKind::Replace,
        version: vec![],
        play_mode: SharedMode::OrderLoop,
        anchor_song_id: String::new(),
        anchor_position: -1,
        display_list: vec!["91".into()],
        random_list: vec!["91".into()],
    }
}
fn pause() -> PlayCommand {
    PlayCommand::at_seconds(CommandKind::Pause, 0.0, PlayState::Pause, "91", "91").unwrap()
}
fn world(api: Arc<ScriptApi>, store: Arc<StateStore>) -> Arc<AccountSession> {
    let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
    session
        .adopt(api, json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    session
}
fn control(session: Arc<AccountSession>) -> TogetherService {
    TogetherService::new(session)
        .unwrap()
        .with_settle_delay(Duration::ZERO)
}

fn peers() -> Reply {
    let mut value = room();
    value["roomUsers"] = json!([{"userId":1}, {"userId":2}]);
    json!({"code":200,"data":{"inRoom":true,"roomInfo":value}}).into()
}
fn remote_command(seq: u64, seconds: f64) -> PlayCommand {
    let mut command =
        PlayCommand::at_seconds(CommandKind::Goto, seconds, PlayState::Pause, "91", "91").unwrap();
    command.user_id = Some("2".into());
    command.server_seq = Some(seq);
    command.client_seq = seq;
    command
}

#[tokio::test]
async fn end_notice_survives_reopen_and_blocks_every_playback_path_without_replaying_writes() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([peers(), snapshot(None), status(false)]));
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let session = world(api.clone(), store.clone());
    let token = session.token().unwrap();
    let service = control(session.clone());
    service.refresh_for(&token, 1000).await.unwrap();
    assert!(
        service
            .receive_end_for(&token, "isolated-room", 2000)
            .unwrap()
    );
    assert!(
        !service
            .receive_end_for(&token, "isolated-room", 3000)
            .unwrap()
    );
    assert!(service.receive_end_for(&token, "other-room", 3000).is_err());
    drop(service);
    drop(session);
    let session = world(api.clone(), store);
    let token = session.token().unwrap();
    let service = control(session.clone());
    assert_eq!(
        service
            .state_for(&token)
            .unwrap()
            .end_notice
            .unwrap()
            .received_at_ms,
        2000
    );
    assert!(!service.tick_for(&token, 900000).await.unwrap());
    assert!(
        service
            .play_for(&token, "isolated-room", pause(), false, 4000)
            .await
            .is_err()
    );
    assert!(
        service
            .queue_for(&token, "isolated-room", list(), 4000)
            .await
            .is_err()
    );
    assert!(
        service
            .receive_play_for(
                &token,
                "isolated-room",
                remote_command(12, 20.0),
                Some("91"),
                true,
                4000
            )
            .is_err()
    );
    assert!(
        service
            .receive_shared_for(&token, "isolated-room", None, false, 4000)
            .await
            .is_err()
    );
    assert_eq!(api.calls().len(), 2);
    service
        .resolve_end_notice_for(&token, "isolated-room", 5000)
        .await
        .unwrap();
    assert!(service.state_for(&token).unwrap().end_notice.is_none());
    assert!(api.calls().iter().all(|c| !c.path.ends_with("end/v2")));
    session
        .adopt(api.clone(), json!({"userId":2}), &token)
        .unwrap();
    assert!(matches!(
        service.receive_end_for(&token, "isolated-room", 6000),
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn interrupted_end_resolution_retains_notice_and_waiting_rooms_ignore_peer_end() {
    let root = tempfile::tempdir().unwrap();
    let (held, release) = gate();
    let api = Arc::new(ScriptApi::new([
        status(true),
        snapshot(None),
        peers(),
        snapshot(None),
        held,
        status(false),
    ]));
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let session = world(api.clone(), store);
    let token = session.token().unwrap();
    let service = control(session.clone());
    service.refresh_for(&token, 1000).await.unwrap();
    assert!(
        !service
            .receive_end_for(&token, "isolated-room", 2000)
            .unwrap()
    );
    service.refresh_for(&token, 3000).await.unwrap();
    assert!(
        service
            .receive_end_for(&token, "isolated-room", 4000)
            .unwrap()
    );
    let task = {
        let session = session.clone();
        let token = token.clone();
        tokio::spawn(async move {
            control(session)
                .resolve_end_notice_for(&token, "isolated-room", 5000)
                .await
        })
    };
    api.wait_for_calls(5).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    drop(release);
    assert!(service.state_for(&token).unwrap().end_notice.is_some());
    service
        .resolve_end_notice_for(&token, "isolated-room", 6000)
        .await
        .unwrap();
    assert!(service.state_for(&token).unwrap().room.is_none());
    assert!(service.operations_for(&token).unwrap().is_empty());
}

#[tokio::test]
async fn incoming_intent_survives_restart_blocks_outgoing_and_has_guarded_completion() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([peers(), snapshot(None), status(false)]));
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let session = world(api.clone(), store.clone());
    let token = session.token().unwrap();
    let service = control(session.clone());
    service.refresh_for(&token, 1000).await.unwrap();
    let command = remote_command(12, 25.0);
    let accepted = service
        .receive_play_for(
            &token,
            "isolated-room",
            command.clone(),
            Some("91"),
            true,
            2000,
        )
        .unwrap()
        .unwrap();
    assert_eq!(accepted.phase, IncomingPhase::Pending);
    drop(service);
    drop(session);
    let session = world(api.clone(), store);
    let token = session.token().unwrap();
    let service = control(session);
    let restored = service.state_for(&token).unwrap().incoming.unwrap();
    assert_eq!(restored.command, command);
    assert_eq!(restored.phase, IncomingPhase::Pending);
    assert!(
        service
            .queue_for(&token, "isolated-room", list(), 3000)
            .await
            .is_err()
    );
    assert_eq!(api.calls().len(), 2);
    assert!(matches!(
        service.finish_incoming_for(&token, "isolated-room", 11, IncomingPhase::Applied),
        Err(Error::StaleResponse)
    ));
    service
        .finish_incoming_for(&token, "isolated-room", 12, IncomingPhase::Failed)
        .unwrap();
    service
        .finish_incoming_for(&token, "isolated-room", 12, IncomingPhase::Applied)
        .unwrap();
    service
        .finish_incoming_for(&token, "isolated-room", 12, IncomingPhase::Applied)
        .unwrap();
    assert!(matches!(
        service.finish_incoming_for(&token, "isolated-room", 12, IncomingPhase::Failed),
        Err(Error::StaleResponse)
    ));
    assert!(
        service
            .receive_play_for(&token, "isolated-room", command, Some("91"), false, 9000)
            .unwrap()
            .is_none()
    );
    assert!(
        service
            .refresh_for(&token, 10000)
            .await
            .unwrap()
            .incoming
            .is_none()
    );
}

#[tokio::test]
async fn room_author_and_sequence_checks_preserve_pending_incoming_commands() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([peers(), snapshot(None)]));
    let session = world(
        api.clone(),
        Arc::new(StateStore::open(root.path()).unwrap()),
    );
    let token = session.token().unwrap();
    let service = control(session.clone());
    service.refresh_for(&token, 1000).await.unwrap();
    let mut command = remote_command(20, 12.0);
    command.user_id = Some("3".into());
    assert!(
        service
            .receive_play_for(
                &token,
                "isolated-room",
                command.clone(),
                Some("91"),
                true,
                2000
            )
            .is_err()
    );
    command.user_id = Some("2".into());
    assert!(
        service
            .receive_play_for(
                &token,
                "other-room",
                command.clone(),
                Some("91"),
                true,
                2000
            )
            .is_err()
    );
    service
        .receive_play_for(&token, "isolated-room", command, Some("91"), true, 2000)
        .unwrap()
        .unwrap();
    assert!(
        service
            .receive_play_for(
                &token,
                "isolated-room",
                remote_command(19, 30.0),
                Some("91"),
                false,
                9000
            )
            .unwrap()
            .is_none()
    );
    let mut newer = remote_command(21, 40.0);
    newer.command_type = CommandKind::Progress;
    service
        .receive_play_for(&token, "isolated-room", newer, Some("91"), false, 9000)
        .unwrap()
        .unwrap();
    assert!(matches!(
        service.finish_incoming_for(&token, "isolated-room", 20, IncomingPhase::Applied),
        Err(Error::StaleResponse)
    ));
    let state = service.state_for(&token).unwrap();
    assert_eq!(state.incoming.unwrap().command.server_seq, Some(21));
    session.adopt(api, json!({"userId":2}), &token).unwrap();
    assert!(matches!(
        service.finish_incoming_for(&token, "isolated-room", 21, IncomingPhase::Applied),
        Err(Error::StaleResponse)
    ));
    assert!(
        service
            .state_for(&session.token().unwrap())
            .unwrap()
            .incoming
            .is_none()
    );
}

#[tokio::test]
async fn room_messages_reset_idle_heartbeat_only_for_the_current_account_and_room() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new(vec![status(true), snapshot(None)]));
    let session = world(
        api.clone(),
        Arc::new(StateStore::open(root.path()).unwrap()),
    );
    let service = control(session.clone());
    let token = session.token().unwrap();
    service.refresh_for(&token, 1000).await.unwrap();
    service
        .message_received_for(&token, "isolated-room", 4000)
        .unwrap();
    assert_eq!(
        service.state_for(&token).unwrap().next_heartbeat_ms,
        Some(24000)
    );
    assert!(
        service
            .message_received_for(&token, "another-room", 5000)
            .is_err()
    );
    assert_eq!(
        service.state_for(&token).unwrap().next_heartbeat_ms,
        Some(24000)
    );
    session.adopt(api, json!({"userId":2}), &token).unwrap();
    assert!(matches!(
        service.message_received_for(&token, "isolated-room", 6000),
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn ended_room_settles_unknown_reports_without_claiming_delivery_or_blocking_future_creation()
{
    for create in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut replies = if create {
            vec![status(false)]
        } else {
            vec![status(true), snapshot(None)]
        };
        replies.extend([
            Reply::Ready(Err(Error::Transport("lost reply".into()))),
            status(false),
            status(false),
        ]);
        let mut next_room = room();
        next_room["roomId"] = json!("next-isolated-room");
        replies.extend([
            status(false),
            json!({"code":200,"data":{"type":"NEW_ROOM","roomInfo":next_room}}).into(),
            json!({"code":200,"data":{"inRoom":true,"roomInfo":next_room}}).into(),
            json!({"code":200,"data":{"inRoom":true,"roomInfo":next_room}}).into(),
        ]);
        let api = Arc::new(ScriptApi::new(replies));
        let s = world(
            api.clone(),
            Arc::new(StateStore::open(root.path()).unwrap()),
        );
        let token = s.token().unwrap();
        let service = control(s);
        if create {
            assert!(service.create_for(&token, "cumulus", 1000).await.is_err());
        } else {
            service.refresh_for(&token, 1000).await.unwrap();
            assert!(
                service
                    .play_for(&token, "isolated-room", pause(), false, 2000)
                    .await
                    .is_err()
            );
        }
        let id = service.operations_for(&token).unwrap()[0].id.clone();
        let result = service.reconcile_for(&token, &id, 3000).await.unwrap();
        assert_eq!(result.phase, TogetherPhase::RoomClosed);
        assert!(!result.acknowledged);
        assert!(service.state_for(&token).unwrap().room.is_none());
        assert!(
            service
                .operations_for(&token)
                .unwrap()
                .iter()
                .all(|o| !o.phase.pending())
        );
        assert_eq!(
            api.calls()
                .iter()
                .filter(
                    |c| c.path.ends_with("room/create") || c.path.ends_with("play/command/report")
                )
                .count(),
            1
        );
        let next = service
            .create_for(&token, "explicit_next_room", 4000)
            .await
            .unwrap();
        assert_eq!(next.phase, TogetherPhase::Confirmed);
        assert_ne!(next.id, id);
        assert_eq!(next.created_room.unwrap().room_id, "next-isolated-room");
    }
}

#[tokio::test]
async fn aborted_write_leaves_durable_intent_and_releases_the_operation_lock() {
    let root = tempfile::tempdir().unwrap();
    let (pending, _send) = gate();
    let api = Arc::new(ScriptApi::new([status(false), pending]));
    let session = world(
        api.clone(),
        Arc::new(StateStore::open(root.path()).unwrap()),
    );
    let token = session.token().unwrap();
    let s = session.clone();
    let t = token.clone();
    let task = tokio::spawn(async move { control(s).create_for(&t, "cumulus", 1000).await });
    api.wait_for_calls(2).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let service = control(session);
    assert_eq!(
        service.operations_for(&token).unwrap()[0].phase,
        TogetherPhase::Sending
    );
    assert!(matches!(
        service.create_for(&token, "cumulus", 2000).await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(api.calls().len(), 2);
}

#[tokio::test]
async fn heartbeat_transport_retry_differs_from_confirmed_room_loss() {
    let root = tempfile::tempdir().unwrap();
    let mut paused = pause();
    paused.user_id = Some("1".into());
    let api = Arc::new(ScriptApi::new([
        status(true),
        snapshot(Some(paused)),
        Reply::Ready(Err(Error::Transport("offline".into()))),
        json!({"code":488,"data":{}}).into(),
        status(false),
    ]));
    let session = world(
        api.clone(),
        Arc::new(StateStore::open(root.path()).unwrap()),
    );
    let token = session.token().unwrap();
    let service = control(session);
    service.refresh_for(&token, 1000).await.unwrap();
    assert!(service.tick_for(&token, 21000).await.is_err());
    assert!(service.state_for(&token).unwrap().room.is_some());
    assert!(!service.tick_for(&token, 21001).await.unwrap());
    assert!(
        service
            .tick_for(&token, 41000)
            .await
            .unwrap_err()
            .is_service_code(488)
    );
    assert!(service.state_for(&token).unwrap().room.is_none());
    assert_eq!(api.calls().len(), 5);
}

#[tokio::test]
async fn timeout_rechecks_membership_before_ending_a_waiting_room() {
    let root = tempfile::tempdir().unwrap();
    let mut joined = room();
    joined["roomUsers"] = json!([{ "userId":1},{"userId":2}]);
    let api = Arc::new(ScriptApi::new([
        status(true),
        snapshot(None),
        json!({"code":200,"data":{"inRoom":true,"roomInfo":joined}}).into(),
        snapshot(None),
    ]));
    let session = world(
        api.clone(),
        Arc::new(StateStore::open(root.path()).unwrap()),
    );
    let token = session.token().unwrap();
    let service = control(session);
    service.refresh_for(&token, 1000).await.unwrap();
    assert!(!service.tick_for(&token, 601000).await.unwrap());
    let state = service.state_for(&token).unwrap();
    assert_eq!(state.role, Some(RoomRole::TogetherOwner));
    assert!(state.waiting_until_ms.is_none());
    assert_eq!(api.calls().len(), 4);
}

#[tokio::test]
async fn explicit_play_rejection_does_not_suppress_the_next_attempt_or_reuse_sequence() {
    let root = tempfile::tempdir().unwrap();
    let mut accepted = pause();
    accepted.client_seq = 2;
    let api = Arc::new(ScriptApi::new([
        status(true),
        snapshot(None),
        json!({"code":403,"data":{}}).into(),
        ack(),
        status(true),
        snapshot(Some(accepted.clone())),
        status(true),
        snapshot(Some(accepted)),
    ]));
    let session = world(
        api.clone(),
        Arc::new(StateStore::open(root.path()).unwrap()),
    );
    let token = session.token().unwrap();
    let service = control(session);
    service.refresh_for(&token, 1000).await.unwrap();
    assert!(
        service
            .play_for(&token, "isolated-room", pause(), false, 2000)
            .await
            .unwrap_err()
            .is_service_code(403)
    );
    assert_eq!(
        service
            .play_for(&token, "isolated-room", pause(), false, 3000)
            .await
            .unwrap()
            .unwrap()
            .phase,
        TogetherPhase::Confirmed
    );
    assert_eq!(service.state_for(&token).unwrap().playback.client_seq, 2);
}

#[tokio::test]
async fn room_queue_pause_heartbeat_and_exit_complete_with_readback() {
    let root = tempfile::tempdir().unwrap();
    let mut paused = pause();
    paused.client_seq = 1;
    let api = Arc::new(ScriptApi::new([
        status(false),
        json!({"code":200,"data":{"type":"NEW_ROOM","roomInfo":room()}}).into(),
        status(true),
        status(true),
        json!({"code":200,"data":{}}).into(),
        ack(),
        status(true),
        snapshot(None),
        status(true),
        snapshot(None),
        ack(),
        status(true),
        snapshot(Some(paused.clone())),
        status(true),
        snapshot(Some(paused)),
        ack(),
        status(true),
        json!({"code":200,"data":{"success":true}}).into(),
        status(false),
        status(false),
    ]));
    let session = world(
        api.clone(),
        Arc::new(StateStore::open(root.path()).unwrap()),
    );
    let token = session.token().unwrap();
    let service = control(session.clone());
    let created = service.create_for(&token, "cumulus", 1000).await.unwrap();
    assert_eq!(created.phase, TogetherPhase::Confirmed);
    assert!(created.created_room.is_some());
    assert_eq!(
        service
            .queue_for(&token, "isolated-room", list(), 2000)
            .await
            .unwrap()
            .phase,
        TogetherPhase::Confirmed
    );
    assert_eq!(
        service
            .play_for(&token, "isolated-room", pause(), false, 3000)
            .await
            .unwrap()
            .unwrap()
            .phase,
        TogetherPhase::Confirmed
    );
    assert!(
        service
            .play_for(&token, "isolated-room", pause(), false, 4000)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!service.tick_for(&token, 20000).await.unwrap());
    assert!(service.tick_for(&token, 22000).await.unwrap());
    assert_eq!(
        service
            .end_for(&token, "isolated-room", None, 23000)
            .await
            .unwrap()
            .phase,
        TogetherPhase::Confirmed
    );
    assert!(service.state_for(&token).unwrap().room.is_none());
    let calls = api.calls();
    assert_eq!(calls.len(), 20);
    assert_eq!(
        calls
            .iter()
            .filter(|c| c.path.ends_with("command/report"))
            .count(),
        2
    );
    assert_eq!(calls[15].payload["progress"], 1);
    assert!(
        service
            .operations_for(&token)
            .unwrap()
            .iter()
            .all(|o| !o.phase.pending())
    );
}
#[tokio::test]
async fn lost_pause_reply_survives_reopen_and_reconciles_without_replay() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let mut paused = pause();
    paused.client_seq = 1;
    let api = Arc::new(ScriptApi::new([
        status(true),
        snapshot(None),
        Reply::Ready(Err(Error::Transport("lost response".into()))),
        status(true),
        snapshot(Some(paused.clone())),
        status(true),
        snapshot(Some(paused)),
    ]));
    let session = world(api.clone(), store.clone());
    let token = session.token().unwrap();
    let service = control(session.clone());
    service.refresh_for(&token, 1000).await.unwrap();
    assert!(
        service
            .play_for(&token, "isolated-room", pause(), false, 2000)
            .await
            .is_err()
    );
    let operation = service.operations_for(&token).unwrap().pop().unwrap();
    assert_eq!(operation.phase, TogetherPhase::Uncertain);
    assert!(matches!(
        service
            .play_for(&token, "isolated-room", pause(), false, 3000)
            .await,
        Err(Error::MutationUncertain)
    ));
    drop(service);
    drop(session);
    drop(store);
    let session = world(
        api.clone(),
        Arc::new(StateStore::open(root.path()).unwrap()),
    );
    let token = session.token().unwrap();
    let service = control(session);
    assert_eq!(
        service
            .reconcile_for(&token, &operation.id, 4000)
            .await
            .unwrap()
            .phase,
        TogetherPhase::Confirmed
    );
    assert_eq!(
        api.calls()
            .iter()
            .filter(|c| c.path.ends_with("play/command/report"))
            .count(),
        1
    );
    assert_eq!(service.state_for(&token).unwrap().playback.client_seq, 1);
}
#[tokio::test]
async fn failed_create_never_claims_an_observed_room_and_existing_room_is_untouched() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([
        status(false),
        Reply::Ready(Err(Error::Transport("lost create".into()))),
        status(true),
        status(true),
        status(true),
        status(true),
    ]));
    let session = world(
        api.clone(),
        Arc::new(StateStore::open(root.path()).unwrap()),
    );
    let token = session.token().unwrap();
    let service = control(session);
    assert!(service.create_for(&token, "cumulus", 1000).await.is_err());
    let id = service.operations_for(&token).unwrap()[0].id.clone();
    let result = service.reconcile_for(&token, &id, 2000).await.unwrap();
    assert_eq!(result.phase, TogetherPhase::Uncertain);
    assert!(result.created_room.is_none());
    assert_eq!(result.observed_room.as_deref(), Some("isolated-room"));
    assert!(matches!(
        service.create_for(&token, "cumulus", 3000).await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(
        api.calls()
            .iter()
            .filter(|c| c.path.ends_with("room/create"))
            .count(),
        1
    );
    assert!(!api.calls().iter().any(|c| c.path.contains("end/v2")));
    let root2 = tempfile::tempdir().unwrap();
    let api2 = Arc::new(ScriptApi::new([status(true)]));
    let s = world(
        api2.clone(),
        Arc::new(StateStore::open(root2.path()).unwrap()),
    );
    let token = s.token().unwrap();
    assert!(
        control(s)
            .create_for(&token, "cumulus", 1000)
            .await
            .is_err()
    );
    assert_eq!(api2.calls().len(), 1);
}
#[tokio::test]
async fn in_flight_switch_keeps_obligation_with_original_account_and_lock_blocks_concurrent_host() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let (pending, send) = gate();
    let api = Arc::new(ScriptApi::new([status(false), pending]));
    let session = world(api.clone(), store.clone());
    let token = session.token().unwrap();
    let work_session = session.clone();
    let work_token = token.clone();
    let task = tokio::spawn(async move {
        control(work_session)
            .create_for(&work_token, "cumulus", 1000)
            .await
    });
    api.wait_for_calls(2).await;
    assert!(matches!(
        control(session.clone())
            .create_for(&token, "cumulus", 1001)
            .await,
        Err(Error::MutationBusy)
    ));
    session
        .adopt(api.clone(), json!({"userId":2}), &token)
        .unwrap();
    send.send(Ok(
        json!({"code":200,"data":{"type":"NEW_ROOM","roomInfo":room()}}),
    ))
    .unwrap();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    let current = session.token().unwrap();
    assert!(
        control(session.clone())
            .operations_for(&current)
            .unwrap()
            .is_empty()
    );
    assert!(control(session).state_for(&current).unwrap().room.is_none());
    let old = store.values("1", "listen-together-operations-v1").unwrap();
    assert_eq!(old.len(), 1);
    assert_eq!(old.values().next().unwrap()["phase"], "uncertain");
}
#[tokio::test]
async fn stale_queue_version_and_wrong_room_are_rejected_before_writing() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([
        status(true),
        snapshot(None),
        snapshot(None),
        status(true),
    ]));
    let s = world(
        api.clone(),
        Arc::new(StateStore::open(root.path()).unwrap()),
    );
    let token = s.token().unwrap();
    let service = control(s);
    service.refresh_for(&token, 1000).await.unwrap();
    assert!(matches!(
        service
            .queue_for(&token, "isolated-room", list(), 2000)
            .await,
        Err(Error::QueueChanged)
    ));
    assert!(
        service
            .end_for(&token, "another-room", None, 3000)
            .await
            .is_err()
    );
    assert_eq!(api.calls().len(), 4);
    assert!(service.operations_for(&token).unwrap().is_empty());
}
