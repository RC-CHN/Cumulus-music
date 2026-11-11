use super::*;
use cumulus_core::listen_together::{RoomRole, TogetherPhase, TogetherService};

pub(super) fn summary() -> Value {
    json!({
        "thisListenCount": 7,
        "thisDuration": 3661.9,
        "totalListenCount": 21,
        "totalDuration": 7390,
        "shareUrl": "https://example.invalid/private-record-link",
        "distance": 122,
        "avatarUrl": "https://example.invalid/untrusted-avatar"
    })
}

fn end() -> TogetherCommand {
    TogetherCommand::End {
        room_id: "test-room".into(),
    }
}

fn open_fixture_room(api: &Backend, peer: bool) {
    let mut state = api.state.lock().unwrap();
    state.inside = true;
    state.peer = peer;
    state.peer_avatar_url = Some("http://p2.music.126.net/peer?token=PRIVATE#secret".into());
}

fn end_calls(api: &Backend) -> Vec<Value> {
    api.calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path.ends_with("/end/v2"))
        .map(|(_, payload)| payload.clone())
        .collect()
}

#[tokio::test]
async fn direct_leave_record_is_projected_once_per_receipt_without_journal_or_share_data() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    open_fixture_room(&api, true);
    *api.end_share_info.lock().unwrap() = Some(summary());
    let (session, client) = world(root.path(), api.clone());
    let before = client.together_view("1", true).await.unwrap().value;
    assert_eq!(before.room.unwrap().role, RoomRole::TogetherOwner);
    assert!(before.leave_record.is_none());

    let view = client.together_command("1", end()).await.unwrap().value;
    assert!(view.room.is_none() && view.pending.is_empty());
    let receipt = view.leave_record.unwrap();
    let id = receipt.operation_id.clone();
    assert_eq!(
        serde_json::to_value(&receipt).unwrap(),
        json!({
            "operationId": id,
            "thisListenCount": 7,
            "thisDuration": 3661,
            "totalListenCount": 21,
            "totalDuration": 7390,
            "selfAvatarUrl": "https://p1.music.126.net/avatar?param=320y320",
            "peerAvatarUrl": "https://p2.music.126.net/peer?param=320y320"
        })
    );
    assert_eq!(
        end_calls(&api),
        vec![json!({"roomId":"test-room","shareInfo":"{\"thisListenCount\":1}"})]
    );
    let service = TogetherService::new(session.clone()).unwrap();
    let durable = service
        .operations_for(&session.token().unwrap())
        .unwrap()
        .into_iter()
        .find(|operation| operation.id == id)
        .unwrap();
    assert!(durable.acknowledged);
    assert_eq!(durable.phase, TogetherPhase::Confirmed);
    let durable = serde_json::to_string(&durable).unwrap();
    for private in [
        "private-record-link",
        "shareUrl",
        "leave_record",
        "thisDuration",
        "this_duration",
        "avatar",
    ] {
        assert!(!durable.contains(private), "{durable}");
    }
    let (_, restarted) = world(root.path(), api.clone());
    assert!(
        restarted
            .together_view("1", false)
            .await
            .unwrap()
            .value
            .leave_record
            .is_none()
    );

    let traffic = api.calls.lock().unwrap().len();
    let dismiss = |operation_id| TogetherCommand::DismissLeaveRecord { operation_id };
    assert!(matches!(
        client
            .together_command("1", dismiss("another-operation".into()))
            .await,
        Err(Error::StaleResponse)
    ));
    let same = client.together_view("1", false).await.unwrap().value;
    assert_eq!(same.leave_record.unwrap().operation_id, id);
    let closed = client
        .together_command("1", dismiss(id.clone()))
        .await
        .unwrap();
    assert!(closed.value.leave_record.is_none());
    assert_eq!(api.calls.lock().unwrap().len(), traffic, "local dismissal");
    let reconciled = client
        .together_command("1", TogetherCommand::Reconcile { operation_id: id })
        .await
        .unwrap();
    assert!(reconciled.value.leave_record.is_none());
    assert_eq!(end_calls(&api).len(), 1);
}

#[tokio::test]
async fn end_record_requires_previous_shared_room_valid_numbers_and_no_replacement() {
    for case in ["waiting", "malformed", "replaced"] {
        let root = tempfile::tempdir().unwrap();
        let api = Arc::new(Backend::default());
        open_fixture_room(&api, case != "waiting");
        let mut reply = summary();
        if case == "malformed" {
            reply["totalListenCount"] = json!("21");
        }
        if case == "replaced" {
            *api.room_after_end.lock().unwrap() = Some("different-room".into());
        }
        *api.end_share_info.lock().unwrap() = Some(reply);
        let (_, client) = world(root.path(), api.clone());
        client.together_view("1", true).await.unwrap();
        let ended = client.together_command("1", end()).await.unwrap().value;
        assert!(ended.leave_record.is_none(), "{case}");
        assert_eq!(
            ended.room.as_ref().map(|room| room.id.as_str()),
            (case == "replaced").then_some("different-room")
        );
        assert_eq!(end_calls(&api).len(), 1);
    }
}

#[tokio::test]
async fn leave_record_does_not_survive_account_epoch_or_next_room() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (session, client) = world(root.path(), api.clone());
    *api.end_share_info.lock().unwrap() = Some(summary());
    for next_room in [true, false] {
        open_fixture_room(&api, true);
        client.together_view("1", true).await.unwrap();
        assert!(
            client
                .together_command("1", end())
                .await
                .unwrap()
                .value
                .leave_record
                .is_some()
        );
        if next_room {
            // A subsequent observed room retires the old receipt even when
            // the transport fixture deliberately reuses the opaque room ID.
            open_fixture_room(&api, false);
            let waiting = client.together_view("1", true).await.unwrap().value;
            assert_eq!(waiting.room.unwrap().role, RoomRole::Waiting);
            assert!(waiting.leave_record.is_none());
            api.state.lock().unwrap().inside = false;
            assert!(
                client
                    .together_view("1", true)
                    .await
                    .unwrap()
                    .value
                    .leave_record
                    .is_none()
            );
        }
    }
    for (uid, epoch) in [(2, "2"), (1, "3")] {
        session
            .adopt(
                api.clone(),
                json!({"userId":uid}),
                &session.token().unwrap(),
            )
            .unwrap();
        assert!(
            client
                .together_view(epoch, false)
                .await
                .unwrap()
                .value
                .leave_record
                .is_none()
        );
    }
    assert!(matches!(
        client.together_view("1", false).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(end_calls(&api).len(), 2);
}

#[tokio::test]
async fn an_end_reply_arriving_after_account_switch_cannot_publish_a_record() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    open_fixture_room(&api, true);
    *api.end_share_info.lock().unwrap() = Some(summary());
    let (session, client) = world(root.path(), api.clone());
    client.together_view("1", true).await.unwrap();
    api.hold_end.store(true, Ordering::SeqCst);
    let (old_result, ()) = tokio::join!(client.together_command("1", end()), async {
        tokio::time::timeout(Duration::from_secs(4), async {
            while !api.end_started.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        session
            .adopt(api.clone(), json!({"userId":2}), &session.token().unwrap())
            .unwrap();
        api.end_release.notify_one();
    });
    assert!(matches!(old_result, Err(Error::StaleResponse)));
    assert!(
        client
            .together_view("2", false)
            .await
            .unwrap()
            .value
            .leave_record
            .is_none()
    );
    session
        .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    let old_room = client.together_view("3", false).await.unwrap().value;
    let operation_id = old_room.pending[0].id.clone();
    assert_eq!(old_room.pending[0].phase, TogetherPhase::Uncertain);
    let reconciled = client
        .together_command("3", TogetherCommand::Reconcile { operation_id })
        .await
        .unwrap()
        .value;
    assert!(reconciled.room.is_none() && reconciled.leave_record.is_none());
    assert_eq!(end_calls(&api).len(), 1);
}

#[tokio::test]
async fn silent_window_cleanup_does_not_display_a_direct_acknowledgement_record() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    incoming_tests::joined(&client, &api, &im).await;
    *api.end_share_info.lock().unwrap() = Some(summary());
    client.close_together().await.unwrap();
    let ended = client.together_view("1", false).await.unwrap().value;
    assert!(ended.room.is_none() && !ended.attached && !ended.local_recovery);
    assert!(ended.leave_record.is_none());
    assert_eq!(end_calls(&api).len(), 1);
}
