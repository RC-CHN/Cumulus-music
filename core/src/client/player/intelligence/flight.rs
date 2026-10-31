use crate::playback::controller::PlaybackSnapshot;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
};
use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Notify;
use tokio::sync::watch;

#[derive(Default)]
pub(in crate::client::player) struct IntelligenceHost {
    pub gate: tokio::sync::Mutex<()>,
    pending: Mutex<Option<Pending>>,
    cancelled_before_open: Mutex<std::collections::VecDeque<(SessionToken, String)>>,
}
struct Pending {
    token: SessionToken,
    id: String,
    cancelled: Arc<AtomicBool>,
    wake: Arc<Notify>,
}
pub(super) struct Flight {
    host: Arc<IntelligenceHost>,
    pub token: SessionToken,
    id: String,
    cancelled: Arc<AtomicBool>,
    wake: Arc<Notify>,
    playback: Mutex<Option<(watch::Receiver<PlaybackSnapshot>, Scope)>>,
}
#[derive(Clone)]
enum Scope {
    Preparing(u64),
    Session(String),
}
impl IntelligenceHost {
    pub(super) fn open(self: &Arc<Self>, token: SessionToken, id: String) -> Result<Flight> {
        uuid::Uuid::parse_str(&id)
            .map_err(|_| Error::InvalidInput("invalid intelligence operation id"))?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let wake = Arc::new(Notify::new());
        let mut slot = self.pending.lock().map_err(|_| Error::LockPoisoned)?;
        let mut early = self
            .cancelled_before_open
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        early.retain(|(owner, _)| owner == &token);
        if let Some(index) = early
            .iter()
            .position(|(owner, key)| owner == &token && key == &id)
        {
            early.remove(index);
            return Err(Error::Cancelled);
        }
        if slot.is_some() {
            return Err(Error::MutationBusy);
        }
        *slot = Some(Pending {
            token: token.clone(),
            id: id.clone(),
            cancelled: cancelled.clone(),
            wake: wake.clone(),
        });
        Ok(Flight {
            host: self.clone(),
            token,
            id,
            cancelled,
            wake,
            playback: Mutex::new(None),
        })
    }
    pub fn cancel(&self, token: &SessionToken, id: &str) -> Result<bool> {
        uuid::Uuid::parse_str(id)
            .map_err(|_| Error::InvalidInput("invalid intelligence operation id"))?;
        let slot = self.pending.lock().map_err(|_| Error::LockPoisoned)?;
        if let Some(p) = slot.as_ref().filter(|p| p.token == *token && p.id == id) {
            p.cancelled.store(true, Ordering::Release);
            p.wake.notify_one();
            return Ok(true);
        }
        // Native IPC handlers can reverse a rapid Start/Cancel pair. Keep a
        // bounded, account-generation-scoped tombstone for that pending read.
        let mut early = self
            .cancelled_before_open
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        early.retain(|(owner, key)| owner == token && key != id);
        if early.len() == 64 {
            early.pop_front();
        }
        early.push_back((token.clone(), id.into()));
        Ok(false)
    }
}
impl Flight {
    pub fn watch_queue(
        &self,
        view: watch::Receiver<PlaybackSnapshot>,
        revision: u64,
    ) -> Result<()> {
        *self.playback.lock().map_err(|_| Error::LockPoisoned)? =
            Some((view, Scope::Preparing(revision)));
        Ok(())
    }
    pub fn watch_session(&self, view: watch::Receiver<PlaybackSnapshot>, id: &str) -> Result<()> {
        *self.playback.lock().map_err(|_| Error::LockPoisoned)? =
            Some((view, Scope::Session(id.into())));
        Ok(())
    }
    pub fn operation_id(&self) -> &str {
        &self.id
    }
    pub fn native_cancel(&self) -> crate::playback::engine::Cancellation {
        let cancelled = self.cancelled.clone();
        crate::playback::engine::Cancellation::new(move || !cancelled.load(Ordering::Acquire))
    }
    pub fn check(&self, session: &AccountSession) -> Result<()> {
        session.check_generation(&self.token)?;
        if self.cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        Ok(())
    }
    pub async fn wait<T>(
        &self,
        session: &AccountSession,
        future: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        let mut account = session.subscribe()?;
        let mut playback = self
            .playback
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .clone();
        self.check(session)?;
        tokio::pin!(future);
        loop {
            if let Some((view, scope)) = &mut playback {
                let state = view.borrow_and_update();
                let matches = match scope {
                    Scope::Preparing(revision) => state.queue_revision == *revision,
                    Scope::Session(id) => state.intelligence.as_ref().is_some_and(|s| &s.id == id),
                };
                if state.account != self.token || !matches {
                    return Err(Error::StaleResponse);
                }
            }
            tokio::select! { biased;
                _=self.wake.notified()=>{self.check(session)?;},
                result=account.changed()=>{result.map_err(|_|Error::Cancelled)?;self.check(session)?;},
                result=async {
                    match &mut playback {
                        Some((view, _)) => view.changed().await.map_err(|_| Error::Cancelled),
                        None => std::future::pending().await,
                    }
                }=>{result?;self.check(session)?;},
                result=&mut future=>{
                    // AccountSession invalidates its generation on its own 301.
                    // Preserve that auth signal; ordinary late results still fail.
                    if result.as_ref().is_err_and(|e|e.is_service_code(301)) {return result;}
                    self.check(session)?;return result;
                }
            }
        }
    }
}
impl Drop for Flight {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.host.pending.lock()
            && slot
                .as_ref()
                .is_some_and(|p| p.token == self.token && p.id == self.id)
        {
            *slot = None;
        }
    }
}
