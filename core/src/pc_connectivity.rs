//! Recovered PC 3.1.42.205594 M/119 connectivity reducer. This model has no
//! I/O and never interprets a media authorization or decoder error.
use crate::{Error, Result};
use std::time::Duration;

const THROTTLE: Duration = Duration::from_millis(100);
const OFFLINE_CHECK: Duration = Duration::from_secs(5);
const RECOVERY_CHECK: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PcRequestObservation {
    /// M/12 emits success before calling the optional response formatter.
    ResolvedBeforeFormatting,
    /// A JSON business response error is not a connectivity event.
    ResponseError,
    FrontendEncryptionError,
    /// The M/12 wrapper rejected with a name other than the two exclusions.
    /// The host must observe that exact boundary, not pass a generic Rust error.
    OtherWrapperRejection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PcConnectivitySignal {
    NativeNotification { online: bool },
    BrowserNotification { online: bool },
    Request(PcRequestObservation),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PcConnectivityEffect {
    ConfirmedOnline(bool),
    /// Restart M/236 playlist-count polling now (the poller cancels its old
    /// timer). Never route this through the player URL or an arbitrary URL.
    RestartPlaylistCountPollingNow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Online,
    PreOffline { since: Duration },
    Offline,
    PreOnline,
}

#[derive(Clone, Copy)]
enum Event {
    TriggerOnline = 0,
    BusinessOnline = 1,
    TriggerOffline = 2,
    BusinessOffline = 3,
}

/// Model of the effective connectivity read by M/23.Z and mirrored into the
/// app store. This is neither media authorization nor the skip experiment.
pub struct PcConnectivity {
    phase: Phase,
    now: Duration,
    last_event: [Option<Duration>; 4],
    probe: Option<Duration>,
    // M/119 publishes false synchronously into switchMap before arming the
    // probe of the retired inner scan. The new scan cannot cancel that probe.
    retired_probe: Option<Duration>,
}

impl PcConnectivity {
    pub fn new(initial_navigator_online: bool, now: Duration) -> Self {
        Self {
            phase: if initial_navigator_online {
                Phase::Online
            } else {
                Phase::Offline
            },
            now,
            last_event: [None; 4],
            probe: None,
            retired_probe: None,
        }
    }

    pub fn is_online(&self) -> bool {
        matches!(self.phase, Phase::Online | Phase::PreOffline { .. })
    }

    pub fn next_probe_at(&self) -> Option<Duration> {
        self.probe.into_iter().chain(self.retired_probe).min()
    }

    /// Submit one event in the host's monotonic delivery order. To reproduce a
    /// previously scheduled callback at the same instant, process it first.
    pub fn signal(
        &mut self,
        now: Duration,
        signal: PcConnectivitySignal,
    ) -> Result<Option<PcConnectivityEffect>> {
        self.observe_time(now)?;
        let event = match signal {
            // Recovered M/119:26–28 sends both callback values to p, which is
            // merged with window.online. v is never fed by that callback.
            PcConnectivitySignal::NativeNotification { .. }
            | PcConnectivitySignal::BrowserNotification { online: true } => Event::TriggerOnline,
            PcConnectivitySignal::BrowserNotification { online: false } => Event::TriggerOffline,
            PcConnectivitySignal::Request(PcRequestObservation::ResolvedBeforeFormatting) => {
                Event::BusinessOnline
            }
            PcConnectivitySignal::Request(PcRequestObservation::OtherWrapperRejection) => {
                Event::BusinessOffline
            }
            PcConnectivitySignal::Request(
                PcRequestObservation::ResponseError | PcRequestObservation::FrontendEncryptionError,
            ) => return Ok(None),
        };
        Ok(self.dispatch(event))
    }

    /// Consume one due callback. Browser offline creates the same business
    /// event as M/119's `musicFeOffline` mock; an online browser returns the
    /// narrow M/236 polling effect and still does not confirm connectivity.
    pub fn poll_probe(
        &mut self,
        now: Duration,
        navigator_online: bool,
    ) -> Result<Option<PcConnectivityEffect>> {
        self.observe_time(now)?;
        let Some(due) = self.next_probe_at().filter(|due| *due <= now) else {
            return Ok(None);
        };
        if self.retired_probe == Some(due) {
            self.retired_probe = None;
        } else {
            self.probe = None;
        }
        if navigator_online {
            Ok(Some(PcConnectivityEffect::RestartPlaylistCountPollingNow))
        } else {
            Ok(self.dispatch(Event::BusinessOffline))
        }
    }

    fn observe_time(&mut self, now: Duration) -> Result<()> {
        if now < self.now || now.checked_add(OFFLINE_CHECK).is_none() {
            return Err(Error::InvalidInput("invalid connectivity clock"));
        }
        self.now = now;
        Ok(())
    }

    fn dispatch(&mut self, event: Event) -> Option<PcConnectivityEffect> {
        let last = &mut self.last_event[event as usize];
        if last.is_some_and(|last| self.now - last < THROTTLE) {
            return None;
        }
        *last = Some(self.now);
        use Event::*;
        use Phase::*;
        match (self.phase, event) {
            (Online, BusinessOffline | TriggerOffline) => {
                if matches!(event, BusinessOffline) {
                    self.phase = PreOffline { since: self.now };
                }
                self.probe = Some(self.now + OFFLINE_CHECK);
            }
            (PreOffline { since }, BusinessOffline) if self.now - since > OFFLINE_CHECK => {
                self.confirm(Offline);
                self.retired_probe = Some(self.now + OFFLINE_CHECK);
                return Some(PcConnectivityEffect::ConfirmedOnline(false));
            }
            (PreOffline { .. }, BusinessOffline) => self.probe = Some(self.now + OFFLINE_CHECK),
            (PreOffline { .. }, TriggerOnline | BusinessOnline) => {
                self.phase = Online;
                self.probe = None;
            }
            (Offline | PreOnline, BusinessOnline) => {
                self.confirm(Online);
                return Some(PcConnectivityEffect::ConfirmedOnline(true));
            }
            (Offline, TriggerOnline) => {
                self.phase = PreOnline;
                self.probe = Some(self.now + RECOVERY_CHECK);
            }
            (Offline | PreOnline, TriggerOffline | BusinessOffline) => {
                self.phase = Offline;
                self.probe = Some(self.now + OFFLINE_CHECK);
            }
            _ => {}
        }
        None
    }

    fn confirm(&mut self, phase: Phase) {
        self.phase = phase;
        self.probe = None;
        // The official switchMap starts a fresh set of four subscriptions.
        self.last_event = [None; 4];
    }
}
