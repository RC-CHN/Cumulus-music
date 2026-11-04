//! Non-blocking bridge to the one thread owning Win32 power state.
#[cfg(windows)]
use super::WindowsPowerSink;
use super::{PowerSink, ThreadPower};
use cumulus_core::playback::keep_awake::{PlaybackPowerRequest, PlaybackPowerSink};
use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
};

const CAPACITY: usize = 16;

struct Running {
    commands: SyncSender<PlaybackPowerRequest>,
    thread: JoinHandle<io::Result<()>>,
}

pub struct DesktopPlaybackPower {
    running: Mutex<Option<Running>>,
    closed: Arc<AtomicBool>,
}

impl DesktopPlaybackPower {
    #[cfg(windows)]
    pub fn start_windows() -> io::Result<Self> {
        Self::start_with(WindowsPowerSink::default)
    }

    fn start_with<S: PowerSink>(sink: impl FnOnce() -> S + Send + 'static) -> io::Result<Self> {
        let (commands, receiver) = mpsc::sync_channel(CAPACITY);
        let closed = Arc::new(AtomicBool::new(false));
        let stopping = closed.clone();
        let thread = thread::Builder::new()
            .name("cumulus-audio-power".into())
            .spawn(move || {
                let mut native = ThreadPower::new(sink());
                while let Ok(request) = receiver.recv() {
                    if stopping.load(Ordering::Acquire) {
                        break;
                    }
                    if native.apply(request).is_err() {
                        eprintln!("Cumulus could not apply a playback power request.");
                    }
                }
                native.clear_if_held()
            })?;
        Ok(Self {
            running: Mutex::new(Some(Running { commands, thread })),
            closed,
        })
    }

    pub fn stop(&self) -> io::Result<()> {
        // Discard queued Keep transitions before waiting for the owner.
        self.closed.store(true, Ordering::Release);
        let running = self
            .running
            .lock()
            .map_err(|_| io::Error::other("playback power worker lock poisoned"))?
            .take();
        if let Some(Running { commands, thread }) = running {
            drop(commands);
            thread
                .join()
                .map_err(|_| io::Error::other("playback power worker panicked"))??;
        }
        Ok(())
    }
}

impl PlaybackPowerSink for DesktopPlaybackPower {
    fn submit(&self, request: PlaybackPowerRequest) {
        if !self.available() {
            return;
        }
        let Ok(guard) = self.running.lock() else {
            self.closed.store(true, Ordering::Release);
            return;
        };
        if let Some(running) = guard.as_ref()
            && running.commands.try_send(request).is_err()
        {
            // Failed delivery of Clear cannot leave a hidden Keep. Fail
            // closed and let the native worker release before exiting.
            self.closed.store(true, Ordering::Release);
        }
    }

    fn available(&self) -> bool {
        !self.closed.load(Ordering::Acquire)
    }
}

impl Drop for DesktopPlaybackPower {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PlaybackPowerRequest::{Clear, KeepSystemAndDisplayAwake as Keep};
    use std::{sync::Condvar, time::Duration};

    struct Fake {
        event: mpsc::Sender<(thread::ThreadId, PlaybackPowerRequest)>,
        block: Arc<(Mutex<bool>, Condvar)>,
    }
    impl PowerSink for Fake {
        fn apply(&mut self, request: PlaybackPowerRequest) -> io::Result<()> {
            self.event.send((thread::current().id(), request)).unwrap();
            let (lock, wake) = &*self.block;
            let guard = lock.lock().unwrap();
            drop(
                wake.wait_timeout_while(guard, Duration::from_secs(5), |held| *held)
                    .unwrap(),
            );
            Ok(())
        }
    }

    #[test]
    fn overload_discards_late_keeps_and_clears_on_the_native_owner() {
        let (event, recv) = mpsc::channel();
        let block = Arc::new((Mutex::new(true), Condvar::new()));
        let held = block.clone();
        let worker = DesktopPlaybackPower::start_with(move || Fake { event, block: held }).unwrap();
        worker.submit(Keep);
        let (owner, state) = recv.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(state, Keep);
        assert_ne!(owner, thread::current().id());
        for _ in 0..CAPACITY {
            worker.submit(Keep);
        }
        worker.submit(Clear);
        *block.0.lock().unwrap() = false;
        block.1.notify_all();
        assert!(!worker.available());
        assert_eq!(
            recv.recv_timeout(Duration::from_secs(2)).unwrap(),
            (owner, Clear)
        );
        worker.stop().unwrap();
        assert!(recv.try_recv().is_err());
    }
}
