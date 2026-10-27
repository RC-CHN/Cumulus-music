//! Durable polling reconciliation. No OS watcher events are required for
//! correctness, and stopping/removing a root never deletes user media.
mod reconcile;
mod registry;
mod types;
use super::LocalLibrary;
use crate::{Error, Result, storage::StateStore};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
pub use types::{WatchEvent, WatchRoot, WatchStatus};

const OWNER: &str = "local";
const ROOTS: &str = "library_watch_roots_v1";
struct Signal {
    stopped: bool,
    epoch: u64,
}
struct Inner {
    library: LocalLibrary,
    store: Arc<StateStore>,
    interval: Duration,
    signal: Mutex<Signal>,
    wake: Condvar,
    listeners: Mutex<Vec<SyncSender<WatchEvent>>>,
    progress: Mutex<BTreeMap<String, (u64, crate::library::ScanProgress)>>,
}

pub struct LibraryMonitor {
    inner: Arc<Inner>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl LibraryMonitor {
    pub fn new(library: LocalLibrary, interval: Duration) -> Result<Self> {
        if !(Duration::from_millis(50)..=Duration::from_secs(3600)).contains(&interval) {
            return Err(Error::InvalidInput(
                "library monitor interval must be 50ms to 3600s",
            ));
        }
        let store = library.state_store();
        Ok(Self {
            inner: Arc::new(Inner {
                library,
                store,
                interval,
                signal: Mutex::new(Signal {
                    stopped: false,
                    epoch: 0,
                }),
                wake: Condvar::new(),
                listeners: Mutex::new(vec![]),
                progress: Mutex::new(BTreeMap::new()),
            }),
            worker: Mutex::new(None),
        })
    }
    pub fn roots(&self) -> Result<Vec<WatchRoot>> {
        self.inner.roots()
    }
    pub fn watch(&self, path: impl AsRef<Path>) -> Result<WatchRoot> {
        self.inner.watch(path.as_ref())
    }
    pub fn unwatch(&self, id: &str) -> Result<Option<WatchRoot>> {
        self.inner.unwatch(id)
    }
    pub fn run_once(&self) -> Result<Vec<WatchRoot>> {
        self.inner.run_once()
    }
    /// Wake the existing worker without spawning competing scans.
    pub fn rescan(&self) -> Result<()> {
        self.inner.notify()
    }
    /// Events are hints with a bounded queue. Slow hosts can read roots() to
    /// recover the durable latest state; they never block reconciliation.
    pub fn subscribe(&self) -> Result<Receiver<WatchEvent>> {
        let (send, receive) = mpsc::sync_channel(32);
        self.inner
            .listeners
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .push(send);
        Ok(receive)
    }
    pub fn start(&self) -> Result<()> {
        let mut worker = self.worker.lock().map_err(|_| Error::LockPoisoned)?;
        if worker.as_ref().is_some_and(|w| !w.is_finished()) {
            return Ok(());
        }
        if let Some(old) = worker.take() {
            old.join()
                .map_err(|_| Error::State("library monitor worker stopped unexpectedly"))?;
        }
        {
            let mut signal = self.inner.signal.lock().map_err(|_| Error::LockPoisoned)?;
            signal.stopped = false;
            signal.epoch = signal.epoch.wrapping_add(1);
        }
        let inner = self.inner.clone();
        *worker = Some(
            thread::Builder::new()
                .name("cumulus-library-monitor".into())
                .spawn(move || inner.run_loop())?,
        );
        Ok(())
    }
    pub fn stop(&self) -> Result<()> {
        let mut worker = self.worker.lock().map_err(|_| Error::LockPoisoned)?;
        {
            let mut signal = self.inner.signal.lock().map_err(|_| Error::LockPoisoned)?;
            signal.stopped = true;
            signal.epoch = signal.epoch.wrapping_add(1);
            self.inner.wake.notify_all();
        }
        if let Some(handle) = worker.take() {
            handle
                .join()
                .map_err(|_| Error::State("library monitor worker stopped unexpectedly"))?;
        }
        Ok(())
    }
}
impl Drop for LibraryMonitor {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

impl Inner {
    fn notify(&self) -> Result<()> {
        let mut signal = self.signal.lock().map_err(|_| Error::LockPoisoned)?;
        signal.epoch = signal.epoch.wrapping_add(1);
        self.wake.notify_all();
        Ok(())
    }
    fn emit(&self, event: WatchEvent) {
        if let Ok(mut listeners) = self.listeners.lock() {
            listeners.retain(|send| match send.try_send(event.clone()) {
                Ok(()) | Err(TrySendError::Full(_)) => true,
                Err(TrySendError::Disconnected(_)) => false,
            });
        }
    }
    fn stopped(&self) -> bool {
        self.signal.lock().map(|s| s.stopped).unwrap_or(true)
    }
    fn run_loop(&self) {
        loop {
            let epoch = match self.signal.lock() {
                Ok(s) if !s.stopped => s.epoch,
                _ => return,
            };
            if let Err(error) = self.run_once() {
                self.emit(WatchEvent::Error {
                    message: error.to_string(),
                });
            }
            let signal = match self.signal.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            if signal.stopped {
                return;
            }
            if signal.epoch == epoch
                && self
                    .wake
                    .wait_timeout_while(signal, self.interval, |s| !s.stopped && s.epoch == epoch)
                    .is_err()
            {
                return;
            }
        }
    }
}
