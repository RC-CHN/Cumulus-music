#[path = "support/together_im.rs"]
mod native;
use cumulus_core::{
    account::AccountSession,
    listen_together::{Room, native_im::ImEvent, transport::*},
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
struct Backend(AtomicUsize);
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(path, "/api/middle/im/token/get");
            let n = self.0.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(
                json!({"code":200,"data":{"uid":1,"accId":"im-account","token":format!("fresh-{n}")}}),
            )
        })
    }
}
fn world() -> (Arc<AccountSession>, Arc<Backend>, Arc<native::Device>, Room) {
    let api = Arc::new(Backend(AtomicUsize::new(0)));
    let session = Arc::new(AccountSession::new(api.clone(), None));
    session
        .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    let room=Room::from_official(&json!({"roomId":"own-room","creatorId":1,"chatRoomId":31,"agoraChannelId":41,"roomUsers":[{"userId":1}]})).unwrap();
    (session, api, Arc::new(native::Device::default()), room)
}
async fn event(events: &mut tokio::sync::mpsc::Receiver<TransportEvent>) -> TransportEvent {
    tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap()
}
#[tokio::test]
async fn real_worker_enters_correct_room_copies_events_and_awaits_native_cleanup() {
    let (session, api, device, room) = world();
    let (im, mut events) = ImTransport::start(
        Arc::new(native::Factory(device.clone())),
        session.clone(),
        session.token().unwrap(),
        room,
    );
    assert!(matches!(
        event(&mut events).await,
        TransportEvent::Entered { reconnected: false }
    ));
    assert!(matches!(
        event(&mut events).await,
        TransportEvent::Message(ImEvent::Message { room: 31, .. })
    ));
    assert_eq!(im.view().state, ConnectionState::Connected);
    assert_eq!(im.view().received, 1);
    assert!(events.try_recv().is_err());
    im.close().await.unwrap();
    assert_eq!(device.active.load(Ordering::SeqCst), 0);
    assert_eq!(device.closed.load(Ordering::SeqCst), 1);
    assert_eq!(api.0.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn interrupted_close_keeps_the_native_join_and_repeated_failure_remains_failure() {
    let (session, _, device, room) = world();
    let (im, mut events) = ImTransport::start(
        Arc::new(native::Factory(device.clone())),
        session.clone(),
        session.token().unwrap(),
        room,
    );
    let im = Arc::new(im);
    event(&mut events).await;
    event(&mut events).await;
    device.hold_close.store(true, Ordering::SeqCst);
    device.close_failure.store(true, Ordering::SeqCst);
    let first = {
        let im = im.clone();
        tokio::spawn(async move { im.close().await })
    };
    tokio::time::timeout(Duration::from_secs(4), async {
        while !device.close_started.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    let second = {
        let im = im.clone();
        tokio::spawn(async move { im.close().await })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    let returned_early = second.is_finished();
    device.hold_close.store(false, Ordering::SeqCst);
    assert!(
        !returned_early,
        "cancelled join must not make the next closer claim completion"
    );
    assert!(second.await.unwrap().is_err());
    assert!(im.close().await.is_err());
    assert_eq!(device.closed.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn im_business_attachments_are_bound_to_the_music_room_not_the_sdk_chatroom() {
    use cumulus_core::listen_together::RoomMessage;
    let (session, _, device, room) = world();
    let (im, mut events) = ImTransport::start(
        Arc::new(native::Factory(device.clone())),
        session.clone(),
        session.token().unwrap(),
        room,
    );
    event(&mut events).await;
    event(&mut events).await;
    let messages = [Some("old-room"), None, Some("own-room")]
        .into_iter()
        .map(|id| {
            RoomMessage::from_attachment(
                &json!({"content":{"type":20008,"content":{"roomId":id,"ignoreUserIds":[]}}})
                    .to_string(),
            )
            .unwrap()
            .unwrap()
        })
        .collect();
    device.controls.lock().unwrap()[0]
        .send(ImEvent::Business { messages })
        .unwrap();
    let TransportEvent::Message(ImEvent::Business { messages }) = event(&mut events).await else {
        panic!("business event missing")
    };
    assert_eq!(messages.len(), 1);
    assert!(messages[0].belongs_to("own-room"));
    assert_eq!(im.view().received, 2);
    im.close().await.unwrap();
}
#[tokio::test]
async fn disconnected_transport_closes_then_fetches_fresh_token_without_business_write_replay() {
    let (session, api, device, room) = world();
    let (im, mut events) = ImTransport::start(
        Arc::new(native::Factory(device.clone())),
        session.clone(),
        session.token().unwrap(),
        room,
    );
    event(&mut events).await;
    event(&mut events).await;
    device.controls.lock().unwrap()[0]
        .send(ImEvent::Disconnected)
        .unwrap();
    assert!(matches!(
        event(&mut events).await,
        TransportEvent::Entered { reconnected: true }
    ));
    event(&mut events).await;
    assert_eq!(api.0.load(Ordering::SeqCst), 2);
    assert_eq!(device.peak.load(Ordering::SeqCst), 1);
    assert_eq!(device.closed.load(Ordering::SeqCst), 1);
    im.close().await.unwrap();
    assert_eq!(device.closed.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn account_change_stops_old_transport_and_rejects_late_message() {
    let (session, api, device, room) = world();
    let (im, mut events) = ImTransport::start(
        Arc::new(native::Factory(device.clone())),
        session.clone(),
        session.token().unwrap(),
        room,
    );
    event(&mut events).await;
    event(&mut events).await;
    session
        .adopt(api, json!({"userId":2}), &session.token().unwrap())
        .unwrap();
    let _ = device.controls.lock().unwrap()[0].send(ImEvent::Message {
        room: 31,
        body: json!({"private":"old account"}),
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while im.view().state != ConnectionState::Closed {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(events.recv().await.is_none());
    im.close().await.unwrap();
    assert_eq!(device.active.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn runtime_guard_ends_transport_even_if_public_handle_is_retained() {
    let (session, _, device, room) = world();
    let (im, mut events) = ImTransport::start(
        Arc::new(native::Factory(device.clone())),
        session.clone(),
        session.token().unwrap(),
        room,
    );
    event(&mut events).await;
    event(&mut events).await;
    drop(im.cancel_on_drop());
    assert!(
        tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(device.active.load(Ordering::SeqCst), 0);
    im.close().await.unwrap();
}
#[tokio::test]
async fn cleanup_failure_is_not_hidden_or_followed_by_another_connection() {
    let (session, api, device, room) = world();
    let (im, mut events) = ImTransport::start(
        Arc::new(native::Factory(device.clone())),
        session.clone(),
        session.token().unwrap(),
        room,
    );
    event(&mut events).await;
    event(&mut events).await;
    device.close_failure.store(true, Ordering::SeqCst);
    assert!(im.close().await.is_err());
    assert_eq!(api.0.load(Ordering::SeqCst), 1);
    assert_eq!(device.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn sdk_link_recovery_does_not_start_an_overlapping_client_and_room_exit_is_terminal() {
    let (session, api, device, room) = world();
    let (im, mut events) = ImTransport::start(
        Arc::new(native::Factory(device.clone())),
        session.clone(),
        session.token().unwrap(),
        room,
    );
    event(&mut events).await;
    event(&mut events).await;
    let control = device.controls.lock().unwrap()[0].clone();
    control
        .send(ImEvent::Link {
            room: 31,
            condition: 1,
        })
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while im.view().state != ConnectionState::Reconnecting {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    control
        .send(ImEvent::Enter {
            room: 31,
            step: 5,
            code: 200,
        })
        .unwrap();
    assert!(matches!(
        event(&mut events).await,
        TransportEvent::Entered { reconnected: true }
    ));
    assert_eq!(device.calls.load(Ordering::SeqCst), 1);
    assert_eq!(api.0.load(Ordering::SeqCst), 1);
    control
        .send(ImEvent::Exit {
            room: 31,
            code: 200,
            reason: 1,
        })
        .unwrap();
    assert!(matches!(
        event(&mut events).await,
        TransportEvent::Message(ImEvent::Exit { reason: 1, .. })
    ));
    assert!(events.recv().await.is_none());
    im.close().await.unwrap();
    assert_eq!(device.closed.load(Ordering::SeqCst), 1);
}
