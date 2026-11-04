//! Native M/236 playlist count poller. M/119 can be attached for the lifetime
//! of a verified desktop document without replacing the one count schedule.
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
    pub count_read_in_flight: bool,
}

enum Command {
    Local(Signal),
    BrowserNotification(u64, bool),
    Request(u64, PcRequestObservation),
    DrawerVisible(u64, bool),
    RefreshCounts(u64),
}

/// Native callers must provide observations from the documented boundary.
/// There is no IPC operation that feeds arbitrary transport errors into it.
pub struct PcMessageConnectivity {
    tx: mpsc::Sender<Command>,
    browser: watch::Sender<(u64, Option<bool>)>,
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
        self.start_pc_message_poll(Some(initial_navigator_online))
    }

    /// Official message counter cadence without using any connectivity inputs.
    pub fn start_pc_message_count_polling(&self) -> Result<PcMessageConnectivity> {
        self.start_pc_message_poll(None)
    }

    fn start_pc_message_poll(
        &self,
        initial_navigator_online: Option<bool>,
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
        initial_navigator_online: Option<bool>,
        lease: Lease,
    ) -> Result<Self> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| Error::State("connectivity executor requires an async runtime"))?;
        let accounts = session.subscribe()?;
        let token = accounts.borrow().clone();
        let initial = PcMessageConnectivityView {
            epoch: token.generation.to_string(),
            online: initial_navigator_online.unwrap_or(false),
            counts: None,
            last_count_read_failed: false,
            count_read_in_flight: false,
        };
        let (tx, rx) = mpsc::channel(32);
        let (browser, browser_lifecycle) = watch::channel((0, initial_navigator_online));
        let (publisher, view) = watch::channel(initial.clone());
        let mut state = State {
            session,
            token,
            browser_generation: 0,
            navigator: initial_navigator_online.unwrap_or(false),
            connectivity: initial_navigator_online
                .map(|online| PcConnectivity::new(online, Duration::ZERO)),
            drawer: false,
            due: None,
            read: None,
            view: initial,
        };
        state.rearm(Duration::ZERO);
        let actor = runtime.spawn(async move {
            let _lease = lease;
            state.run(accounts, browser_lifecycle, rx, publisher).await;
        });
        Ok(Self {
            tx,
            browser,
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

    /// Attach an initial browser sample, or detach and discard only its
    /// connectivity timers. The message-count timer is never replaced here.
    pub fn try_browser_lifecycle(&self, initial: Option<bool>) -> Result<()> {
        if self.browser.is_closed() {
            return Err(Error::Cancelled);
        }
        // Lifecycle is independent of the bounded notification mailbox. A
        // retiring document can always revoke its timers before the next tick.
        let mut result = Err(Error::State("browser generation exhausted"));
        self.browser.send_if_modified(|(generation, sample)| {
            let Some(next) = generation.checked_add(1) else {
                return false;
            };
            *generation = next;
            *sample = initial;
            result = Ok(());
            true
        });
        result
    }

    pub fn try_observe_browser(&self, online: bool) -> Result<()> {
        let generation = self.browser.borrow().0;
        self.try_send(Command::BrowserNotification(generation, online))
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

    /// Synchronous native window lifecycle callback, sharing the same mailbox.
    pub fn try_drawer_visible(&self, generation: u64, visible: bool) -> Result<()> {
        self.try_send(Command::DrawerVisible(generation, visible))
    }

    pub fn try_refresh_counts(&self, generation: u64) -> Result<()> {
        self.try_send(Command::RefreshCounts(generation))
    }

    fn try_send(&self, command: Command) -> Result<()> {
        self.tx.try_send(command).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => Error::MutationBusy,
            mpsc::error::TrySendError::Closed(_) => Error::Cancelled,
        })
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
    browser_generation: u64,
    navigator: bool,
    connectivity: Option<PcConnectivity>,
    drawer: bool,
    due: Option<Duration>,
    read: Option<InFlight>,
    view: PcMessageConnectivityView,
}

impl State {
    async fn run(
        mut self,
        mut accounts: watch::Receiver<SessionToken>,
        mut browser: watch::Receiver<(u64, Option<bool>)>,
        mut commands: mpsc::Receiver<Command>,
        publisher: watch::Sender<PcMessageConnectivityView>,
    ) {
        let start = Instant::now();
        loop {
            let wake = self
                .due
                .into_iter()
                .chain(
                    self.connectivity
                        .as_ref()
                        .and_then(PcConnectivity::next_probe_at),
                )
                .min();
            tokio::select! {
                biased;
                changed = accounts.changed() => {
                    if changed.is_err() { break; }
                    self.bind(accounts.borrow_and_update().clone(), start.elapsed());
                }
                changed = browser.changed() => {
                    if changed.is_err() { break; }
                    let (generation, initial) = *browser.borrow_and_update();
                    self.browser_generation = generation;
                    self.navigator = initial.unwrap_or(false);
                    self.view.online = self.navigator;
                    self.connectivity = initial.map(|online| PcConnectivity::new(online, start.elapsed()));
                }
                () = wait_for_timer(wake.map(|offset| start + offset)) => self.timer(start.elapsed()),
                result = wait_for_read(&mut self.read) => self.finish(result, start.elapsed()),
                command = commands.recv() => match command {
                    Some(Command::Local(signal)) => self.observe(signal, start.elapsed()),
                    Some(Command::BrowserNotification(generation, online))
                        if generation == self.browser_generation && self.connectivity.is_some() =>
                    {
                        self.observe(Signal::BrowserNotification { online }, start.elapsed());
                    }
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
        self.view.count_read_in_flight = false;
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
        if let Some(model) = self.connectivity.as_mut()
            && let Ok(Some(effect)) = model.signal(now, signal)
        {
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
        if let Some(model) = self.connectivity.as_mut()
            && let Ok(Some(effect)) = model.poll_probe(now, self.navigator)
        {
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
        self.view.count_read_in_flight = true;
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
        self.view.count_read_in_flight = false;
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
