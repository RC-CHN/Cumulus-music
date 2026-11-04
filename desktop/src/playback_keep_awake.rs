//! Dedicated Windows thread for the recovered playback power requests.
#![cfg(any(test, windows))]

mod delivery;
#[cfg(windows)]
pub use delivery::DesktopPlaybackPower;

use cumulus_core::playback::keep_awake::PlaybackPowerRequest;
use std::{
    io,
    sync::mpsc,
    thread::{self, JoinHandle},
};

const fn execution_flags(request: PlaybackPowerRequest) -> u32 {
    // The recovered Windows handler uses ES_CONTINUOUS on both transitions.
    // The audio effect never generates partial requirements or away mode.
    match request {
        PlaybackPowerRequest::Clear => 0x8000_0000,
        PlaybackPowerRequest::KeepSystemAndDisplayAwake => 0x8000_0003,
    }
}

trait PowerSink {
    fn apply(&mut self, request: PlaybackPowerRequest) -> io::Result<()>;
}

#[cfg(windows)]
#[derive(Default)]
struct WindowsPowerSink(std::marker::PhantomData<std::rc::Rc<()>>);

#[cfg(windows)]
impl PowerSink for WindowsPowerSink {
    fn apply(&mut self, request: PlaybackPowerRequest) -> io::Result<()> {
        use windows_sys::Win32::System::Power::SetThreadExecutionState;

        // SAFETY: the valid flag mask has no pointer arguments. This sink is
        // constructed and used only by the dedicated execution-state thread.
        let previous = unsafe { SetThreadExecutionState(execution_flags(request)) };
        if previous == 0 {
            let error = io::Error::last_os_error();
            return Err(if error.raw_os_error() == Some(0) {
                io::Error::other("SetThreadExecutionState returned zero")
            } else {
                error
            });
        }
        Ok(())
    }
}

struct Command {
    request: PlaybackPowerRequest,
    reply: mpsc::Sender<io::Result<()>>,
}

struct Running {
    commands: mpsc::SyncSender<Command>,
    thread: JoinHandle<io::Result<()>>,
}

/// Owns one dedicated operating-system thread for thread-scoped power state.
/// Each call waits for its exact result. An observer should own this host off
/// the UI thread; no playback-state subscription is installed here.
pub struct PlaybackKeepAwakeHost {
    running: Option<Running>,
}

impl PlaybackKeepAwakeHost {
    /// Start a Windows worker without issuing a power request.
    #[cfg(windows)]
    pub fn start_windows() -> io::Result<Self> {
        Self::start_with(WindowsPowerSink::default)
    }

    fn start_with<S: PowerSink>(
        make_sink: impl FnOnce() -> S + Send + 'static,
    ) -> io::Result<Self> {
        // Rendezvous, with an explicit result per command. There is no queue
        // of stale transitions and different states cannot overwrite one another.
        let (commands, inbox) = mpsc::sync_channel::<Command>(0);
        let thread = thread::Builder::new()
            .name("cumulus-playback-power".into())
            .spawn(move || {
                // S need not be Send: only its factory crosses the thread boundary.
                let mut worker = ThreadPower::new(make_sink());
                for command in inbox {
                    let _ = command.reply.send(worker.apply(command.request));
                }
                worker.clear_if_held()
            })?;
        Ok(Self {
            running: Some(Running { commands, thread }),
        })
    }

    /// Apply one observation, preserving different request states in order.
    /// Repeating the last successful state is safe only absent a failure.
    pub fn apply(&mut self, request: PlaybackPowerRequest) -> io::Result<()> {
        fn closed() -> io::Error {
            io::Error::new(io::ErrorKind::BrokenPipe, "playback power worker closed")
        }
        let running = self.running.as_ref().ok_or_else(closed)?;
        let (reply, result) = mpsc::channel();
        running
            .commands
            .send(Command { request, reply })
            .map_err(|_| closed())?;
        result.recv().map_err(|_| closed())?
    }

    /// Join after same-thread cleanup and report any shutdown error.
    pub fn shutdown(mut self) -> io::Result<()> {
        self.finish()
    }

    fn finish(&mut self) -> io::Result<()> {
        let Some(Running { commands, thread }) = self.running.take() else {
            return Ok(());
        };
        drop(commands);
        thread
            .join()
            .map_err(|_| io::Error::other("playback power worker panicked"))?
    }
}

impl Drop for PlaybackKeepAwakeHost {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

struct ThreadPower<S: PowerSink> {
    sink: S,
    known: Option<PlaybackPowerRequest>,
    may_hold: bool,
}

impl<S: PowerSink> ThreadPower<S> {
    fn new(sink: S) -> Self {
        Self {
            sink,
            known: None,
            may_hold: false,
        }
    }

    fn apply(&mut self, request: PlaybackPowerRequest) -> io::Result<()> {
        if self.known == Some(request) {
            return Ok(());
        }
        // Keep the cleanup obligation even if a requested clear fails or a
        // sink unwinds after attempting to acquire the thread requirement.
        self.known = None;
        if request == PlaybackPowerRequest::KeepSystemAndDisplayAwake {
            self.may_hold = true;
        }
        self.sink.apply(request)?;
        self.known = Some(request);
        self.may_hold = request == PlaybackPowerRequest::KeepSystemAndDisplayAwake;
        Ok(())
    }

    fn clear_if_held(&mut self) -> io::Result<()> {
        if std::mem::take(&mut self.may_hold) {
            // One cleanup attempt. Neither an error nor Drop repeats it.
            self.sink.apply(PlaybackPowerRequest::Clear)?;
        }
        Ok(())
    }
}

impl<S: PowerSink> Drop for ThreadPower<S> {
    fn drop(&mut self) {
        let _ = self.clear_if_held();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PlaybackPowerRequest::{Clear, KeepSystemAndDisplayAwake as Keep};
    use std::{
        collections::VecDeque,
        rc::Rc,
        sync::{Arc, Mutex},
        thread::ThreadId,
    };

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Event {
        Created(ThreadId),
        Apply(ThreadId, PlaybackPowerRequest),
        Dropped(ThreadId),
    }

    enum Outcome {
        Applied,
        Failed(i32),
        Panic,
    }

    struct FakeSink {
        // Intentionally !Send, like the Windows sink.
        owner: Rc<ThreadId>,
        trace: Arc<Mutex<Vec<Event>>>,
        outcomes: VecDeque<Outcome>,
    }

    impl FakeSink {
        fn record(&self, event: Event) {
            assert_eq!(*self.owner, thread::current().id());
            self.trace.lock().unwrap().push(event);
        }
    }

    impl PowerSink for FakeSink {
        fn apply(&mut self, request: PlaybackPowerRequest) -> io::Result<()> {
            self.record(Event::Apply(thread::current().id(), request));
            match self.outcomes.pop_front().unwrap_or(Outcome::Applied) {
                Outcome::Applied => Ok(()),
                Outcome::Failed(code) => Err(io::Error::from_raw_os_error(code)),
                Outcome::Panic => panic!("synthetic sink unwind"),
            }
        }
    }

    impl Drop for FakeSink {
        fn drop(&mut self) {
            self.record(Event::Dropped(thread::current().id()));
        }
    }

    fn fake(
        outcomes: impl Into<VecDeque<Outcome>>,
    ) -> (PlaybackKeepAwakeHost, Arc<Mutex<Vec<Event>>>) {
        let trace = Arc::new(Mutex::new(Vec::new()));
        let sink_trace = trace.clone();
        let outcomes = outcomes.into();
        let host = PlaybackKeepAwakeHost::start_with(move || {
            let owner = thread::current().id();
            let sink = FakeSink {
                owner: Rc::new(owner),
                trace: sink_trace,
                outcomes,
            };
            sink.record(Event::Created(owner));
            sink
        })
        .unwrap();
        (host, trace)
    }

    fn assert_worker_trace(trace: &Mutex<Vec<Event>>, requests: &[PlaybackPowerRequest]) {
        let trace = trace.lock().unwrap();
        let Some(Event::Created(owner)) = trace.first().copied() else {
            panic!("missing worker creation");
        };
        assert_ne!(owner, thread::current().id());
        let mut expected = vec![Event::Created(owner)];
        expected.extend(requests.iter().map(|request| Event::Apply(owner, *request)));
        expected.push(Event::Dropped(owner));
        assert_eq!(*trace, expected);
    }

    #[test]
    fn playback_requests_match_the_recovered_windows_masks() {
        assert_eq!(execution_flags(Clear), 0x8000_0000);
        assert_eq!(execution_flags(Keep), 0x8000_0003);
    }

    #[test]
    fn idle_start_and_drop_do_not_issue_a_power_request() {
        let (host, trace) = fake([]);
        drop(host);
        assert_worker_trace(&trace, &[]);
    }

    #[test]
    fn only_equal_successful_requests_coalesce_and_transitions_are_ordered() {
        let (mut host, trace) = fake([]);
        for request in [Clear, Clear, Keep, Keep, Clear, Keep] {
            host.apply(request).unwrap();
        }
        host.shutdown().unwrap();
        assert_worker_trace(&trace, &[Clear, Keep, Clear, Keep, Clear]);
    }

    #[test]
    fn explicit_successful_clear_needs_no_duplicate_drop_clear() {
        let (mut host, trace) = fake([]);
        host.apply(Keep).unwrap();
        host.apply(Clear).unwrap();
        drop(host);
        assert_worker_trace(&trace, &[Keep, Clear]);
    }

    #[test]
    fn failed_clear_invalidates_the_prior_successful_state() {
        let (mut host, trace) = fake([Outcome::Applied, Outcome::Failed(5)]);
        host.apply(Keep).unwrap();
        assert_eq!(host.apply(Clear).unwrap_err().raw_os_error(), Some(5));
        host.apply(Keep).unwrap();
        host.shutdown().unwrap();
        assert_worker_trace(&trace, &[Keep, Clear, Keep, Clear]);
    }

    #[test]
    fn failed_keep_is_not_retried_and_drop_clears_possible_hold() {
        let (mut host, trace) = fake([Outcome::Failed(5)]);
        assert_eq!(host.apply(Keep).unwrap_err().raw_os_error(), Some(5));
        assert_eq!(trace.lock().unwrap().len(), 2); // creation and one failed Keep
        drop(host);
        assert_worker_trace(&trace, &[Keep, Clear]);
    }

    #[test]
    fn shutdown_reports_failed_clear_and_does_not_retry() {
        let (mut host, trace) = fake([Outcome::Applied, Outcome::Failed(5)]);
        host.apply(Keep).unwrap();
        assert_eq!(host.shutdown().unwrap_err().raw_os_error(), Some(5));
        assert_worker_trace(&trace, &[Keep, Clear]);
    }

    #[test]
    fn worker_unwind_clears_and_drops_on_its_own_thread() {
        let (mut host, trace) = fake([Outcome::Panic]);
        assert_eq!(
            host.apply(Keep).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(host.shutdown().unwrap_err().kind(), io::ErrorKind::Other);
        assert_worker_trace(&trace, &[Keep, Clear]);
    }
}
