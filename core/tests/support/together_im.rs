use cumulus_core::{
    Error, Result,
    account::{AccountSession, SessionToken},
    listen_together::{
        ImCredentials, Room,
        native_im::ImEvent,
        transport::{ImConnection, ImFactory},
    },
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};
#[derive(Default)]
pub struct Device {
    pub calls: AtomicUsize,
    pub closed: AtomicUsize,
    pub active: AtomicUsize,
    pub peak: AtomicUsize,
    pub close_failure: AtomicBool,
    pub hold_close: AtomicBool,
    pub close_started: AtomicBool,
    pub controls: Mutex<Vec<Sender<ImEvent>>>,
}
pub struct Factory(pub Arc<Device>);
impl ImFactory for Factory {
    fn connect(
        &self,
        _session: Arc<AccountSession>,
        token: SessionToken,
        credentials: ImCredentials,
    ) -> Result<Box<dyn ImConnection>> {
        assert_eq!(token.uid.as_deref(), Some(credentials.uid.as_str()));
        let device = self.0.clone();
        device.calls.fetch_add(1, Ordering::SeqCst);
        let active = device.active.fetch_add(1, Ordering::SeqCst) + 1;
        device.peak.fetch_max(active, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        sender
            .send(ImEvent::Login {
                step: 3,
                code: 200,
                retrying: false,
            })
            .unwrap();
        device.controls.lock().unwrap().push(sender.clone());
        Ok(Box::new(Connection {
            device,
            sender,
            receiver,
            closed: false,
        }))
    }
}
struct Connection {
    device: Arc<Device>,
    sender: Sender<ImEvent>,
    receiver: Receiver<ImEvent>,
    closed: bool,
}
impl ImConnection for Connection {
    fn enter(&mut self, room: &Room) -> Result<()> {
        let id = room.chat_room_id.parse::<i64>().unwrap();
        self.sender
            .send(ImEvent::Enter {
                room: id + 1,
                step: 5,
                code: 200,
            })
            .unwrap();
        self.sender.send(ImEvent::Grant { code: 200 }).unwrap();
        self.sender
            .send(ImEvent::Enter {
                room: id,
                step: 5,
                code: 200,
            })
            .unwrap();
        self.sender
            .send(ImEvent::Message {
                room: id + 1,
                body: serde_json::json!({"secret":"wrong room"}),
            })
            .unwrap();
        self.sender
            .send(ImEvent::Message {
                room: id,
                body: serde_json::json!({"msg_type":5,"private":"never serialize"}),
            })
            .unwrap();
        Ok(())
    }
    fn next_event(&mut self, timeout: Duration) -> Result<Option<ImEvent>> {
        Ok(self.receiver.recv_timeout(timeout).ok())
    }
    fn close(&mut self) -> Result<()> {
        if !self.closed {
            self.device.close_started.store(true, Ordering::SeqCst);
            while self.device.hold_close.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(2));
            }
            std::thread::sleep(Duration::from_millis(40));
            self.closed = true;
            self.device.closed.fetch_add(1, Ordering::SeqCst);
            self.device.active.fetch_sub(1, Ordering::SeqCst);
        }
        if self.device.close_failure.load(Ordering::SeqCst) {
            Err(Error::State("injected cleanup failure"))
        } else {
            Ok(())
        }
    }
}
