//! Single-video actor: network/decoder loads are cancellable, account-bound,
//! and independent of the frame-render worker and any Tauri host.
mod actions;
mod actor;
mod local;
mod resolve;
mod types;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    playback::engine::{Cancellation, VideoEngine, VideoFrame},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::sync::{mpsc, oneshot, watch};
pub use types::{VideoCommand, VideoSnapshot, VideoStatus};
pub type VideoEngineFactory = Arc<dyn Fn() -> Result<Box<dyn VideoEngine>> + Send + Sync>;
type Factory = VideoEngineFactory;
// Field order matters: destroy the native decoder before releasing its file.
struct HeldEngine {
    device: Box<dyn VideoEngine>,
    _lease: Option<crate::storage::OperationLock>,
}
type Engine = Arc<Mutex<HeldEngine>>;
type FrameSlot = Arc<Mutex<Option<(String, Engine)>>>;
enum Message {
    Command(VideoCommand, oneshot::Sender<Result<VideoSnapshot>>),
    Loading(u64),
    Local(u64, local::LocalSource),
    Loaded(u64, Box<Result<resolve::Prepared>>),
    Shutdown,
}
pub struct VideoController {
    session: Arc<AccountSession>,
    token: SessionToken,
    sender: mpsc::UnboundedSender<Message>,
    view: watch::Receiver<VideoSnapshot>,
    frames: FrameSlot,
    stop: Arc<AtomicBool>,
    finished: watch::Receiver<bool>,
}
impl VideoController {
    pub fn start(
        session: Arc<AccountSession>,
        token: SessionToken,
        factory: Factory,
    ) -> Result<Self> {
        session.with_token(&token, |_| Ok(()))?;
        tokio::runtime::Handle::try_current()
            .map_err(|_| Error::State("video requires an async runtime"))?;
        let frames = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::unbounded_channel();
        let actor = actor::Actor::new(
            session.clone(),
            token.clone(),
            factory,
            sender.clone(),
            frames.clone(),
            stop.clone(),
        )?;
        let (publisher, view) = watch::channel(actor.view.clone());
        let (finished_sender, finished) = watch::channel(false);
        tokio::spawn(async move {
            actor.run(receiver, publisher).await;
            finished_sender.send_replace(true);
        });
        Ok(Self {
            session,
            token,
            sender,
            view,
            frames,
            stop,
            finished,
        })
    }
    pub fn snapshot(&self) -> Result<VideoSnapshot> {
        self.session
            .with_token(&self.token, |_| Ok(self.view.borrow().clone()))
    }
    pub fn subscribe(&self) -> watch::Receiver<VideoSnapshot> {
        self.view.clone()
    }
    /// Native host retirement remains valid after account expiry and waits for
    /// the old active engine to be released before another account can play.
    pub async fn shutdown(&self) -> Result<()> {
        self.stop.store(true, Ordering::Release);
        let _ = self.sender.send(Message::Shutdown);
        let mut finished = self.finished.clone();
        while !*finished.borrow_and_update() {
            finished.changed().await.map_err(|_| Error::Cancelled)?;
        }
        Ok(())
    }
    pub async fn command(
        &self,
        token: &SessionToken,
        command: VideoCommand,
    ) -> Result<VideoSnapshot> {
        if token != &self.token {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(token, |_| Ok(()))?;
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(Message::Command(command, sender))
            .map_err(|_| Error::Cancelled)?;
        let result = receiver.await.map_err(|_| Error::Cancelled)??;
        self.session.with_token(token, |_| Ok(result))
    }
    pub fn frame(
        &self,
        token: &SessionToken,
        play_id: &str,
        after: u64,
        width: u32,
        height: u32,
    ) -> Result<Option<Arc<VideoFrame>>> {
        if !(16..=1920).contains(&width) || !(16..=1080).contains(&height) {
            return Err(Error::InvalidInput("invalid video surface dimensions"));
        }
        if token != &self.token {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(token, |_| Ok(()))?;
        let engine = self
            .frames
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .filter(|(id, _)| id == play_id)
            .map(|(_, e)| e.clone());
        let Some(engine) = engine else {
            return Ok(None);
        };
        let frame = match engine.try_lock() {
            Ok(engine) => {
                engine.device.resize(width, height)?;
                engine.device.frame_after(after)?
            }
            Err(std::sync::TryLockError::WouldBlock) => None,
            Err(_) => return Err(Error::LockPoisoned),
        };
        self.session.with_token(token, |_| Ok(()))?;
        if self
            .frames
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .is_none_or(|(id, _)| id != play_id)
        {
            return Ok(None);
        }
        Ok(frame)
    }
}
impl Drop for VideoController {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.sender.send(Message::Shutdown);
    }
}
async fn device<T: Send + 'static>(
    engine: Engine,
    operation: impl FnOnce(&mut dyn VideoEngine) -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(move || {
        operation(
            engine
                .lock()
                .map_err(|_| Error::LockPoisoned)?
                .device
                .as_mut(),
        )
    })
    .await
    .map_err(|_| Error::State("video worker failed"))?
}
