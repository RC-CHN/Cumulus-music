//! Native M/236 playlist count poller. M/119 can be attached for the lifetime
//! of a verified desktop document without replacing the one count schedule.
mod playback;
mod read_all;
use super::{AccountData, MessageCounts, MusicClient};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    pc_connectivity::{
        PcConnectivity, PcConnectivityEffect as Effect, PcConnectivitySignal as Signal,
        PcRequestObservation,
    },
};
use playback::{PlaybackConnectivity, Sample};
use read_all::{CountSample, ReadAll};
pub use read_all::{MessageReadAllAction, MessageReadAllState};
use serde::Deserialize;
use std::{
    future::{Future, pending},
    pin::Pin,
    sync::{
        Arc, Mutex,
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
pub(in crate::client) struct PcMessagePollHost {
    active: Arc<AtomicBool>,
    playback: Mutex<Option<PlaybackConnectivity>>,
}

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
    pub hide_private_row_badges: bool,
    pub last_count_read_failed: bool,
    pub count_read_in_flight: bool,
}

/// Committed navigation inside a visible message drawer. Private's official
/// tab key is numeric 0, so it never clears the host's string `msg` category.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MessageCountNavigation {
    Private,
    Comments,
    Forwards,
    Notices,
    Conversation,
}

enum Command {
    Local(Signal),
    BrowserNotification(u64, bool),
    Request(u64, PcRequestObservation),
    DrawerVisible(u64, bool),
    DrawerNavigated(u64, MessageCountNavigation),
    RefreshCounts(u64),
    ReadAll(u64, MessageReadAllAction, read_all::Reply),
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
        let mut playback = self
            .message_poll
            .playback
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        let guard = self.message_poll.active.clone();
        if guard.swap(true, Ordering::AcqRel) {
            return Err(Error::MutationBusy);
        }
        let (poller, binding) = PcMessageConnectivity::start(
            self.session.clone(),
            initial_navigator_online,
            Lease(guard),
        )?;
        *playback = Some(binding);
        Ok(poller)
    }
}
impl PcMessagePollHost {
    pub(in crate::client) fn playback_authority(
        &self,
        token: &SessionToken,
    ) -> Result<Option<crate::playback::engine::Cancellation>> {
        Ok(self
            .playback
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .and_then(|binding| binding.authority(token)))
    }
}

impl PcMessageConnectivity {
    fn start(
        session: Arc<AccountSession>,
        initial_navigator_online: Option<bool>,
        lease: Lease,
    ) -> Result<(Self, PlaybackConnectivity)> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| Error::State("connectivity executor requires an async runtime"))?;
        let accounts = session.subscribe()?;
        let token = accounts.borrow().clone();
        let initial = PcMessageConnectivityView {
            epoch: token.generation.to_string(),
            online: initial_navigator_online.unwrap_or(false),
            counts: None,
            hide_private_row_badges: false,
            last_count_read_failed: false,
            count_read_in_flight: false,
        };
        let (tx, rx) = mpsc::channel(32);
        let (browser, browser_lifecycle) = watch::channel((0, initial_navigator_online));
        let (publisher, view) = watch::channel(initial.clone());
        let (sample_tx, samples) = watch::channel(Sample {
            account: token.generation,
            document: 0,
            allowed: false,
        });
        let binding = PlaybackConnectivity {
            browser: browser.subscribe(),
            samples,
        };
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
            read_all: ReadAll::default(),
            view: initial,
        };
        state.rearm(Duration::ZERO);
        let actor = runtime.spawn(async move {
            let _lease = lease;
            state
                .run(accounts, browser_lifecycle, rx, publisher, sample_tx)
                .await;
        });
        Ok((
            Self {
                tx,
                browser,
                view,
                actor: Some(actor),
            },
            binding,
        ))
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

    /// Native main-window adapters use this existing poller; no second count
    /// reader or renderer-provided account ID is used for read-all recovery.
    pub fn message_read_all(
        &self,
        epoch: &str,
        action: MessageReadAllAction,
    ) -> impl Future<Output = Result<AccountData<MessageReadAllState>>> + Send + use<> {
        let generation = epoch.parse();
        let tx = self.tx.clone();
        async move {
            let generation =
                generation.map_err(|_| Error::InvalidInput("invalid message count epoch"))?;
            let (send, receive) = tokio::sync::oneshot::channel();
            tx.send(Command::ReadAll(generation, action, send))
                .await
                .map_err(|_| Error::Cancelled)?;
            receive.await.map_err(|_| Error::Cancelled)?
        }
    }

    /// Synchronous native window lifecycle callback, sharing the same mailbox.
    pub fn try_drawer_visible(&self, generation: u64, visible: bool) -> Result<()> {
        self.try_send(Command::DrawerVisible(generation, visible))
    }

    /// A tab change or conversation enter/return reuses the visible poller.
    pub fn try_drawer_navigated(
        &self,
        generation: u64,
        navigation: MessageCountNavigation,
    ) -> Result<()> {
        self.try_send(Command::DrawerNavigated(generation, navigation))
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

type ReadResult = (SessionToken, Result<CountSample>);
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
    read_all: ReadAll,
    view: PcMessageConnectivityView,
}

impl State {
    async fn run(
        mut self,
        mut accounts: watch::Receiver<SessionToken>,
        mut browser: watch::Receiver<(u64, Option<bool>)>,
        mut commands: mpsc::Receiver<Command>,
        publisher: watch::Sender<PcMessageConnectivityView>,
        samples: watch::Sender<Sample>,
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
            let mut completion = None;
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
                result = read_all::wait_for_work(&mut self.read_all.work) => {
                    completion = Some(self.finish_read_all(result));
                }
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
                        if !visible {
                            self.read_all.revoke_intent();
                        }
                        self.rearm(start.elapsed());
                    }
                    Some(Command::DrawerNavigated(generation, navigation))
                        if self.drawer && self.is_current_account(generation) =>
                    {
                        self.navigate_drawer(navigation, start.elapsed());
                    }
                    Some(Command::RefreshCounts(generation)) if self.is_current_account(generation) => {
                        self.start_read();
                    }
                    Some(Command::ReadAll(generation, action, reply)) => {
                        completion = self.read_all_action(generation, action, reply);
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            let sample = Sample {
                account: self.token.generation,
                document: self.browser_generation,
                // The browser initial sample is not proof of a usable PC
                // transport. Use the existing successful authenticated read
                // and deny while this partial signal set is ambiguous.
                allowed: self.token.uid.is_some()
                    && self.connectivity.is_some()
                    && self.navigator
                    && self.view.online
                    && self.view.counts.is_some()
                    && !self.view.last_count_read_failed,
            };
            samples.send_if_modified(|current| {
                if *current == sample {
                    false
                } else {
                    *current = sample;
                    true
                }
            });
            publisher.send_if_modified(|current| {
                if current == &self.view {
                    false
                } else {
                    *current = self.view.clone();
                    true
                }
            });
            if let Some((reply, result)) = completion {
                let _ = reply.send(result);
            }
        }
    }

    fn bind(&mut self, token: SessionToken, now: Duration) {
        if self.token == token {
            return;
        }
        self.token = token;
        self.read = None;
        self.read_all = ReadAll::default();
        self.drawer = false;
        self.rearm(now);
        self.view.epoch = self.token.generation.to_string();
        self.view.counts = self.token.uid.is_none().then(read_all::zero_counts);
        self.view.hide_private_row_badges = false;
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

    fn navigate_drawer(&mut self, navigation: MessageCountNavigation, now: Duration) {
        use MessageCountNavigation::{Comments, Conversation, Forwards, Notices, Private};
        if let Some(counts) = self.view.counts.as_mut() {
            match navigation {
                Comments => counts.comments = Some(0),
                Forwards => counts.forwards = Some(0),
                Notices => counts.notices = Some(0),
                Private | Conversation => {}
            }
        }
        // This is only local display state: no new signed observation, no
        // write, and no durable settlement of a read-all operation.
        self.read_all.revoke_intent();
        self.rearm(now);
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
        if self.read.is_some() || self.read_all.work.is_some() || self.token.uid.is_none() {
            return;
        }
        let token = self.token.clone();
        let session = self.session.clone();
        self.view.count_read_in_flight = true;
        self.read = Some(Box::pin(async move {
            let result = tokio::time::timeout(REQUEST_TIMEOUT, CountSample::read(&session, &token))
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
            Ok(mut sample) => {
                self.read_all.observe(Some(&mut sample));
                self.view.hide_private_row_badges = sample.hide_private_row_badges;
                self.view.counts = Some(sample.counts);
                self.view.last_count_read_failed = false;
                self.observe(
                    Signal::Request(PcRequestObservation::ResolvedBeforeFormatting),
                    now,
                );
            }
            Err(_) => {
                self.read_all.observe(None);
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
