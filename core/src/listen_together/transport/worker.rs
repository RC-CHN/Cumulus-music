use super::*;
use crate::listen_together::TogetherApi;
use std::time::Instant;

fn state(sender: &watch::Sender<ConnectionView>, next: ConnectionState, message: Option<&str>) {
    sender.send_modify(|v| {
        v.state = next;
        v.message = message.map(str::to_owned);
    });
}
async fn cancelled(receiver: &mut watch::Receiver<bool>) {
    while !*receiver.borrow_and_update() {
        if receiver.changed().await.is_err() {
            return;
        }
    }
}
struct Cancellation(Arc<AtomicBool>);
impl Drop for Cancellation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
pub(super) struct Context {
    pub factory: Arc<dyn ImFactory>,
    pub session: Arc<AccountSession>,
    pub token: SessionToken,
    pub room: Room,
    pub stop: Arc<AtomicBool>,
    pub cancel: watch::Receiver<bool>,
    pub view: watch::Sender<ConnectionView>,
    pub events: mpsc::Sender<TransportEvent>,
}
impl Context {
    pub async fn run(self) -> Result<()> {
        let Self {
            factory,
            session,
            token,
            room,
            stop,
            mut cancel,
            view,
            events,
        } = self;
        let _cancel_on_drop = Cancellation(stop.clone());
        // Network/grant expiry gets fresh credentials and bounded retries. A new
        // explicit UI retry starts another supervisor; it never resends HTTP writes.
        for attempt in 1..=4 {
            if stop.load(Ordering::Acquire) || session.with_token(&token, |_| Ok(())).is_err() {
                break;
            }
            view.send_modify(|v| {
                v.attempt = attempt;
                v.state = if attempt == 1 {
                    ConnectionState::Connecting
                } else {
                    ConnectionState::Reconnecting
                };
                v.message = None;
            });
            let credentials = tokio::select! {
                biased;
                _=cancelled(&mut cancel)=>break,
                value=session.run_for(&token,|api,_|async move{TogetherApi::new(api).im_credentials().await})=>value,
            };
            let result = match credentials {
                Ok(credentials) => {
                    let failure_view = view.clone();
                    let (factory, session, token, room, stop, view, events) = (
                        factory.clone(),
                        session.clone(),
                        token.clone(),
                        room.clone(),
                        stop.clone(),
                        view.clone(),
                        events.clone(),
                    );
                    tokio::task::spawn_blocking(move || {
                        connection(
                            factory,
                            session,
                            token,
                            room,
                            credentials,
                            stop,
                            view,
                            events,
                            attempt > 1,
                        )
                    })
                    .await
                    .map_err(|_| {
                        state(
                            &failure_view,
                            ConnectionState::Failed,
                            Some("消息连接异常，请重新启动客户端"),
                        );
                        Error::State("IM worker failed")
                    })?
                }
                Err(Error::StaleResponse) => break,
                Err(error) => {
                    // Session expiry and explicit service refusal require a user
                    // recovery action, unlike transport interruptions.
                    if !matches!(error.root(), Error::Transport(_)) {
                        state(
                            &view,
                            ConnectionState::Failed,
                            Some("消息连接授权失败，请刷新登录后重试"),
                        );
                        return Ok(());
                    }
                    Outcome::Retry
                }
            };
            match result {
                Outcome::Stopped => break,
                Outcome::Fatal => {
                    state(
                        &view,
                        ConnectionState::Failed,
                        Some("消息连接失败，请重新连接"),
                    );
                    return Ok(());
                }
                Outcome::CleanupFailed => {
                    state(
                        &view,
                        ConnectionState::Failed,
                        Some("消息连接未能关闭，请重启客户端"),
                    );
                    return Err(Error::State("IM cleanup unresolved"));
                }
                Outcome::Retry if attempt < 4 => {
                    state(
                        &view,
                        ConnectionState::Reconnecting,
                        Some("消息连接中断，正在重试"),
                    );
                    tokio::select! { biased; _=cancelled(&mut cancel)=>break, _=tokio::time::sleep(Duration::from_secs(u64::from(attempt)*2))=>{} }
                }
                Outcome::Retry => {
                    state(
                        &view,
                        ConnectionState::Failed,
                        Some("消息连接中断，请重新连接"),
                    );
                    return Ok(());
                }
            }
        }
        state(&view, ConnectionState::Closed, None);
        Ok(())
    }
}
enum Outcome {
    Stopped,
    Retry,
    Fatal,
    CleanupFailed,
}
fn transient(code: i32) -> bool {
    matches!(code, 302 | 408 | 415 | 10001)
}
#[allow(clippy::too_many_arguments)]
fn connection(
    factory: Arc<dyn ImFactory>,
    session: Arc<AccountSession>,
    token: SessionToken,
    room: Room,
    credentials: ImCredentials,
    stop: Arc<AtomicBool>,
    view: watch::Sender<ConnectionView>,
    events: mpsc::Sender<TransportEvent>,
    reconnected: bool,
) -> Outcome {
    if stop.load(Ordering::Acquire) {
        return Outcome::Stopped;
    }
    let mut connection = match factory.connect(session.clone(), token.clone(), credentials) {
        Ok(value) => value,
        Err(Error::StaleResponse) => return Outcome::Stopped,
        Err(_) => return Outcome::Fatal,
    };
    let mut entering = false;
    let mut connected = false;
    let mut entered_once = false;
    let mut deadline = Instant::now() + Duration::from_secs(45);
    let outcome = loop {
        if stop.load(Ordering::Acquire) || session.with_token(&token, |_| Ok(())).is_err() {
            break Outcome::Stopped;
        }
        if !connected && Instant::now() > deadline {
            break Outcome::Retry;
        }
        let event = match connection.next_event(Duration::from_millis(100)) {
            Ok(Some(event)) => event,
            Ok(None) => continue,
            Err(Error::StaleResponse) => break Outcome::Stopped,
            Err(_) => break Outcome::Retry,
        };
        if stop.load(Ordering::Acquire) || session.with_token(&token, |_| Ok(())).is_err() {
            break Outcome::Stopped;
        }
        match event {
            ImEvent::Login {
                step: 3, code: 200, ..
            } if !entering => {
                if connection.enter(&room).is_err() {
                    break Outcome::Fatal;
                }
                entering = true;
                state(&view, ConnectionState::Entering, None);
            }
            ImEvent::Login {
                code,
                retrying: false,
                ..
            } if code != 200 => {
                break if transient(code) {
                    Outcome::Retry
                } else {
                    Outcome::Fatal
                };
            }
            ImEvent::Grant { code } if code != 200 => {
                break if transient(code) {
                    Outcome::Retry
                } else {
                    Outcome::Fatal
                };
            }
            ImEvent::Enter {
                room: id,
                step,
                code,
            } => {
                if id.to_string() != room.chat_room_id {
                    continue;
                }
                if code != 200 {
                    break if transient(code) {
                        Outcome::Retry
                    } else {
                        Outcome::Fatal
                    };
                }
                if step == 5 {
                    if events
                        .try_send(TransportEvent::Entered {
                            reconnected: reconnected || entered_once,
                        })
                        .is_err()
                    {
                        break Outcome::Retry;
                    }
                    connected = true;
                    entered_once = true;
                    state(&view, ConnectionState::Connected, None);
                }
            }
            ImEvent::Message { room: id, .. } | ImEvent::Notification { room: id, .. }
                if id.to_string() == room.chat_room_id && connected =>
            {
                if matches!(event, ImEvent::Message { .. }) {
                    view.send_modify(|v| v.received = v.received.saturating_add(1));
                }
                if events.try_send(TransportEvent::Message(event)).is_err() {
                    break Outcome::Retry;
                }
            }
            ImEvent::Business { mut messages } if connected => {
                messages.retain(|m| m.belongs_to(&room.room_id));
                if !messages.is_empty() {
                    view.send_modify(|v| {
                        v.received = v.received.saturating_add(messages.len() as u64)
                    });
                    if events
                        .try_send(TransportEvent::Message(ImEvent::Business { messages }))
                        .is_err()
                    {
                        break Outcome::Retry;
                    }
                }
            }
            ImEvent::Disconnected => break Outcome::Retry,
            ImEvent::Kicked { .. } => break Outcome::Fatal,
            ImEvent::Exit {
                room: id, reason, ..
            } if id.to_string() == room.chat_room_id => {
                let _ = events.try_send(TransportEvent::Message(event));
                break match reason {
                    0 | 1 => Outcome::Stopped,
                    4 => Outcome::Retry,
                    _ => Outcome::Fatal,
                };
            }
            ImEvent::Link {
                room: id,
                condition,
            } if id.to_string() == room.chat_room_id && condition != 0 => {
                state(
                    &view,
                    ConnectionState::Reconnecting,
                    Some("消息连接正在恢复"),
                );
                if condition == 1 {
                    if connected {
                        deadline = Instant::now() + Duration::from_secs(45);
                    }
                    connected = false;
                } else {
                    break Outcome::Retry;
                }
            }
            _ => {}
        }
    };
    state(&view, ConnectionState::Closing, None);
    if connection.close().is_err() {
        Outcome::CleanupFailed
    } else {
        outcome
    }
}
