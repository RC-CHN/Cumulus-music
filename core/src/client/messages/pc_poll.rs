//! Native, opt-in executor for M/119 and the shared M/236 playlist-count
//! poller. No desktop or renderer initializes it implicitly.
use super::{MessageCounts, MusicClient, read_counts};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    pc_connectivity::{
        PcConnectivity, PcConnectivityEffect as Effect, PcConnectivitySignal as Signal,
        PcRequestObservation,
    },
};
use std::{
    future::{Future, pending},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
    time::Instant,
};

const ORDINARY: Duration = Duration::from_secs(240);
const DRAWER: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(in crate::client) struct PcMessagePollHost(Arc<AtomicBool>);

struct Lease(Arc<AtomicBool>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PcMessageConnectivityView {
    pub epoch: String,
    pub online: bool,
    pub counts: Option<MessageCounts>,
    pub last_count_read_failed: bool,
}

enum Command {
    Local(Signal),
    Request(u64, PcRequestObservation),
    DrawerVisible(u64, bool),
    RefreshCounts(u64),
}

/// Native callers must provide observations from the documented boundary.
/// There is no IPC operation that feeds arbitrary transport errors into it.
pub struct PcMessageConnectivity {
    tx: mpsc::Sender<Command>,
    view: watch::Receiver<PcMessageConnectivityView>,
    actor: Option<JoinHandle<()>>,
}

impl MusicClient {
    /// Explicit native embedding only. The caller owns its lifetime and must
    /// close it when the authoritative window/event sources are disposed.
    pub fn start_pc_message_connectivity(
        &self,
        initial_navigator_online: bool,
    ) -> Result<PcMessageConnectivity> {
        let guard = self.message_poll.0.clone();
        if guard.swap(true, Ordering::AcqRel) {
            return Err(Error::MutationBusy);
        }
        PcMessageConnectivity::start(self.session.clone(), initial_navigator_online, Lease(guard))
    }
}

impl PcMessageConnectivity {
    fn start(
        session: Arc<AccountSession>,
        initial_navigator_online: bool,
        lease: Lease,
    ) -> Result<Self> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| Error::State("connectivity executor requires an async runtime"))?;
        let accounts = session.subscribe()?;
        let token = accounts.borrow().clone();
        let initial = PcMessageConnectivityView {
            epoch: token.generation.to_string(),
            online: initial_navigator_online,
            counts: None,
            last_count_read_failed: false,
        };
        let (tx, rx) = mpsc::channel(32);
        let (publisher, view) = watch::channel(initial.clone());
        let mut state = State {
            session,
            token,
            navigator: initial_navigator_online,
            connectivity: PcConnectivity::new(initial_navigator_online, Duration::ZERO),
            drawer: false,
            due: None,
            read: None,
            view: initial,
        };
        state.rearm(Duration::ZERO);
        let actor = runtime.spawn(async move {
            let _lease = lease;
            state.run(accounts, rx, publisher).await;
        });
        Ok(Self {
            tx,
            view,
            actor: Some(actor),
        })
    }

    pub fn subscribe(&self) -> watch::Receiver<PcMessageConnectivityView> {
        self.view.clone()
    }

    pub async fn observe_native(&self, online: bool) -> Result<()> {
        self.send(Command::Local(Signal::NativeNotification { online }))
            .await
    }

    pub async fn observe_browser(&self, online: bool) -> Result<()> {
        self.send(Command::Local(Signal::BrowserNotification { online }))
            .await
    }

    pub async fn observe_request(
        &self,
        generation: u64,
        observation: PcRequestObservation,
    ) -> Result<()> {
        self.send(Command::Request(generation, observation)).await
    }

    /// This is the actual message sheet visibility, not OS focus or a
    /// reachability signal. Closing its surface returns to the 240s interval.
    pub async fn drawer_visible(&self, generation: u64, visible: bool) -> Result<()> {
        self.send(Command::DrawerVisible(generation, visible)).await
    }

    /// Explicit counter refresh (or a separately proven matching IM notice).
    /// This does not rearm the periodic timer and never reads message bodies.
    pub async fn refresh_counts(&self, generation: u64) -> Result<()> {
        self.send(Command::RefreshCounts(generation)).await
    }

    async fn send(&self, command: Command) -> Result<()> {
        self.tx.send(command).await.map_err(|_| Error::Cancelled)
    }

    pub async fn shutdown(mut self) {
        if let Some(actor) = self.actor.take() {
            actor.abort();
            let _ = actor.await;
        }
    }
}

impl Drop for PcMessageConnectivity {
    fn drop(&mut self) {
        if let Some(actor) = self.actor.take() {
            actor.abort();
        }
    }
}

type ReadResult = (SessionToken, Result<MessageCounts>);
type InFlight = Pin<Box<dyn Future<Output = ReadResult> + Send>>;

struct State {
    session: Arc<AccountSession>,
    token: SessionToken,
    navigator: bool,
    connectivity: PcConnectivity,
    drawer: bool,
    due: Option<Duration>,
    read: Option<InFlight>,
    view: PcMessageConnectivityView,
}

impl State {
    async fn run(
        mut self,
        mut accounts: watch::Receiver<SessionToken>,
        mut commands: mpsc::Receiver<Command>,
        publisher: watch::Sender<PcMessageConnectivityView>,
    ) {
        let start = Instant::now();
        loop {
            let wake = self
                .due
                .into_iter()
                .chain(self.connectivity.next_probe_at())
                .min();
            tokio::select! {
                biased;
                changed = accounts.changed() => {
                    if changed.is_err() { break; }
                    self.bind(accounts.borrow_and_update().clone(), start.elapsed());
                }
                () = wait_for_timer(wake.map(|offset| start + offset)) => self.timer(start.elapsed()),
                result = wait_for_read(&mut self.read) => self.finish(result, start.elapsed()),
                command = commands.recv() => match command {
                    Some(Command::Local(signal)) => self.observe(signal, start.elapsed()),
                    Some(Command::Request(generation, observation))
                        if generation == self.token.generation && self.session.check_generation(&self.token).is_ok() =>
                    {
                        self.observe(Signal::Request(observation), start.elapsed());
                    }
                    Some(Command::DrawerVisible(generation, visible))
                        if self.is_current_account(generation) && self.drawer != visible =>
                    {
                        self.drawer = visible;
                        self.rearm(start.elapsed());
                    }
                    Some(Command::RefreshCounts(generation)) if self.is_current_account(generation) => {
                        self.start_read();
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            publisher.send_if_modified(|current| {
                if current == &self.view {
                    false
                } else {
                    *current = self.view.clone();
                    true
                }
            });
        }
    }

    fn bind(&mut self, token: SessionToken, now: Duration) {
        if self.token == token {
            return;
        }
        self.token = token;
        self.read = None;
        self.drawer = false;
        self.rearm(now);
        self.view.epoch = self.token.generation.to_string();
        self.view.counts = self.token.uid.is_none().then_some(MessageCounts {
            private: Some(0),
            comments: Some(0),
            forwards: Some(0),
            notices: Some(0),
        });
        self.view.last_count_read_failed = false;
    }

    fn is_current_account(&self, generation: u64) -> bool {
        self.token.uid.is_some()
            && generation == self.token.generation
            && self.session.check_generation(&self.token).is_ok()
    }

    fn observe(&mut self, signal: Signal, now: Duration) {
        if let Signal::BrowserNotification { online } = signal {
            self.navigator = online;
        }
        if let Ok(Some(effect)) = self.connectivity.signal(now, signal) {
            self.apply(effect, now);
        }
    }

    fn apply(&mut self, effect: Effect, now: Duration) {
        match effect {
            Effect::ConfirmedOnline(online) => self.view.online = online,
            Effect::RestartPlaylistCountPollingNow => self.rearm(now),
        }
    }

    fn rearm(&mut self, now: Duration) {
        self.due = self.token.uid.is_some().then_some(now);
    }

    fn timer(&mut self, now: Duration) {
        if let Ok(Some(effect)) = self.connectivity.poll_probe(now, self.navigator) {
            self.apply(effect, now);
        }
        if self.due.is_some_and(|deadline| deadline <= now) {
            self.due = Some(now + if self.drawer { DRAWER } else { ORDINARY });
            self.start_read();
        }
    }

    fn start_read(&mut self) {
        if self.read.is_some() || self.token.uid.is_none() {
            return;
        }
        let token = self.token.clone();
        let session = self.session.clone();
        self.read = Some(Box::pin(async move {
            let result = tokio::time::timeout(REQUEST_TIMEOUT, read_counts(&session, &token))
                .await
                .unwrap_or(Err(Error::State("message count request timed out")));
            (token, result)
        }));
    }

    fn finish(&mut self, (token, result): ReadResult, now: Duration) {
        self.read = None;
        if token != self.token || self.session.check_generation(&token).is_err() {
            return;
        }
        match result {
            Ok(counts) => {
                self.view.counts = Some(counts);
                self.view.last_count_read_failed = false;
                self.observe(
                    Signal::Request(PcRequestObservation::ResolvedBeforeFormatting),
                    now,
                );
            }
            Err(_) => {
                // The validated Rust endpoint doesn't expose the M/12 wrapper's
                // named failure categories. Retain counts and do not infer
                // offline from HTTP status, expiry, cancellation or timeout.
                self.view.last_count_read_failed = true;
            }
        }
    }
}

async fn wait_for_read(read: &mut Option<InFlight>) -> ReadResult {
    match read {
        Some(read) => read.await,
        None => pending().await,
    }
}

async fn wait_for_timer(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => pending().await,
    }
}
