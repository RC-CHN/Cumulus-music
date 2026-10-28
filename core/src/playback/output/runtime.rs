use super::{OutputPreferences, OutputSelection};
use crate::{
    Error, Result,
    playback::engine::{OutputDevice, OutputDevices},
    storage::StateStore,
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    Audio,
    Video,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputPhase {
    Dormant,
    Pending,
    Unsupported,
    Configured,
    Fallback,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputIssue {
    MissingDevice,
    Initialization,
    Selection,
    Enumeration,
    DamagedPreferences,
    Storage,
}

#[derive(Clone, Debug)]
pub struct OutputObservation {
    pub kind: OutputKind,
    pub revision: Option<u64>,
    pub phase: OutputPhase,
    pub issue: Option<OutputIssue>,
    pub configured: Option<String>,
    pub driver: Option<String>,
}
pub struct OutputSnapshot {
    pub preferences: Option<OutputPreferences>,
    pub damaged: bool,
    pub supported: Option<bool>,
    pub devices: Vec<OutputDevice>,
    pub engines: Vec<OutputObservation>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Key {
    pub revision: Option<u64>,
    pub id: String,
}
impl Key {
    pub fn from(preferences: &Option<OutputPreferences>) -> Self {
        Self {
            revision: preferences.as_ref().map(|p| p.revision),
            id: preferences
                .as_ref()
                .map_or("auto", |p| p.selection.id())
                .into(),
        }
    }
}
#[derive(Clone)]
pub(super) struct Policy {
    pub key: Key,
    pub fallback: Option<OutputIssue>,
    pub recovery_used: bool,
    pub initialization_failed: bool,
}
impl Policy {
    pub fn effective(&self) -> &str {
        if self.fallback.is_some() {
            "auto"
        } else {
            &self.key.id
        }
    }
}
struct Slot {
    instance: u64,
    policy: Option<Policy>,
    observed: OutputObservation,
}
#[derive(Default)]
struct State {
    next: u64,
    slots: BTreeMap<OutputKind, Slot>,
    devices: Vec<OutputDevice>,
    supported: Option<bool>,
}
pub struct OutputRuntime {
    pub(super) store: Arc<StateStore>,
    state: Mutex<State>,
    changed: AtomicU64,
}
pub struct OutputLease {
    pub(super) runtime: Arc<OutputRuntime>,
    pub(super) kind: OutputKind,
    instance: u64,
}
impl OutputRuntime {
    pub fn new(store: Arc<StateStore>) -> Arc<Self> {
        Arc::new(Self {
            store,
            state: Mutex::new(State::default()),
            changed: AtomicU64::new(0),
        })
    }
    pub fn preferences(&self) -> Result<Option<OutputPreferences>> {
        match OutputPreferences::read(&self.store) {
            Ok(value) => Ok(Some(value)),
            Err(Error::Json(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }
    pub fn snapshot(&self) -> Result<OutputSnapshot> {
        let preferences = self.preferences()?;
        let key = Key::from(&preferences);
        let state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        let engines = state
            .slots
            .values()
            .map(|slot| {
                let mut observed = slot.observed.clone();
                if observed.phase != OutputPhase::Dormant
                    && slot.policy.as_ref().is_none_or(|p| p.key != key)
                {
                    observed.phase = OutputPhase::Pending;
                }
                observed
            })
            .collect();
        Ok(OutputSnapshot {
            damaged: preferences.is_none(),
            preferences,
            engines,
            devices: state.devices.clone(),
            supported: state.supported,
        })
    }
    /// A settings catalog may use a separate, never-loaded native engine. This
    /// avoids acquiring a decoder mutex while a cancellable load is pending.
    pub fn catalog(&self, devices: Option<OutputDevices>) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        state.supported = Some(devices.is_some());
        state.devices = devices.map_or_else(Vec::new, |d| d.devices);
        Ok(())
    }
    pub fn select(&self, expected: u64, id: &str) -> Result<OutputPreferences> {
        let selection = if id == "auto" {
            OutputSelection::default()
        } else {
            let state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
            let device = state
                .devices
                .iter()
                .find(|d| d.id == id)
                .ok_or(Error::State("audio device is no longer available"))?;
            OutputSelection::new(device.id.clone(), device.description.clone())?
        };
        let value = OutputPreferences::replace(&self.store, expected, selection)?;
        self.notify();
        Ok(value)
    }
    pub fn retry(&self, expected: u64) -> Result<OutputPreferences> {
        let current = OutputPreferences::read(&self.store)?;
        if current.revision != expected {
            return Err(Error::StaleResponse);
        }
        let value = OutputPreferences::replace(&self.store, expected, current.selection)?;
        self.notify();
        Ok(value)
    }
    pub fn repair(&self) -> Result<OutputPreferences> {
        let value = OutputPreferences::repair(&self.store)?;
        self.notify();
        Ok(value)
    }
    fn notify(&self) {
        // Polling still reads persistence once per second; saturation cannot
        // silently lose a preference even after this process-local hint ends.
        let _ = self
            .changed
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1));
    }
    pub(super) fn version(&self) -> u64 {
        self.changed.load(Ordering::Acquire)
    }
    /// Reserve before constructing the native engine. A slow superseded MV
    /// constructor must not overwrite the newer engine's status when it returns.
    pub fn reserve(self: &Arc<Self>, kind: OutputKind) -> Result<OutputLease> {
        let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        let instance = state
            .next
            .checked_add(1)
            .ok_or(Error::State("audio output instance exhausted"))?;
        state.next = instance;
        let policy = state.slots.get(&kind).and_then(|s| s.policy.clone());
        state.slots.insert(
            kind,
            Slot {
                instance,
                policy,
                observed: OutputObservation {
                    kind,
                    revision: None,
                    phase: OutputPhase::Pending,
                    issue: None,
                    configured: None,
                    driver: None,
                },
            },
        );
        Ok(OutputLease {
            runtime: self.clone(),
            kind,
            instance,
        })
    }
}
impl OutputLease {
    fn with_slot<T>(&self, operation: impl FnOnce(&mut Slot) -> Result<T>) -> Result<T> {
        let mut state = self.runtime.state.lock().map_err(|_| Error::LockPoisoned)?;
        let slot = state
            .slots
            .get_mut(&self.kind)
            .filter(|s| s.instance == self.instance)
            .ok_or(Error::Cancelled)?;
        operation(slot)
    }
    pub fn valid(&self) -> bool {
        self.with_slot(|_| Ok(())).is_ok()
    }
    pub(super) fn cancellation(
        &self,
        parent: &crate::playback::engine::Cancellation,
    ) -> crate::playback::engine::Cancellation {
        let parent = parent.clone();
        let runtime = Arc::downgrade(&self.runtime);
        let kind = self.kind;
        let instance = self.instance;
        crate::playback::engine::Cancellation::new(move || {
            !parent.is_cancelled()
                && runtime.upgrade().is_some_and(|runtime| {
                    runtime.state.lock().is_ok_and(|state| {
                        state
                            .slots
                            .get(&kind)
                            .is_some_and(|s| s.instance == instance)
                    })
                })
        })
    }
    pub(super) fn policy(&self, preferences: &Option<OutputPreferences>) -> Result<Policy> {
        let key = Key::from(preferences);
        self.with_slot(|slot| {
            if slot.policy.as_ref().is_none_or(|p| p.key != key) {
                slot.policy = Some(Policy {
                    key,
                    fallback: preferences
                        .is_none()
                        .then_some(OutputIssue::DamagedPreferences),
                    recovery_used: false,
                    initialization_failed: false,
                });
            }
            Ok(slot.policy.clone().expect("policy initialized"))
        })
    }
    pub(super) fn fallback(&self, key: &Key, issue: OutputIssue, recovery: bool) -> Result<Policy> {
        self.with_slot(|slot| {
            let policy = slot
                .policy
                .as_mut()
                .filter(|p| &p.key == key)
                .ok_or(Error::StaleResponse)?;
            if recovery && policy.recovery_used {
                return Err(Error::State("audio output recovery exhausted"));
            }
            policy.fallback = Some(issue);
            policy.recovery_used |= recovery;
            Ok(policy.clone())
        })
    }
    pub(super) fn observe(&self, key: &Key, observed: OutputObservation) -> Result<()> {
        self.with_slot(|slot| {
            if slot.policy.as_ref().is_none_or(|p| &p.key != key) {
                return Err(Error::StaleResponse);
            }
            slot.observed = observed;
            Ok(())
        })
    }
    pub(super) fn initialization(&self, key: &Key, failed: bool) -> Result<Policy> {
        self.with_slot(|slot| {
            let policy = slot
                .policy
                .as_mut()
                .filter(|p| &p.key == key)
                .ok_or(Error::StaleResponse)?;
            policy.initialization_failed = failed;
            Ok(policy.clone())
        })
    }
    pub(super) fn devices(&self, devices: &[OutputDevice]) -> Result<()> {
        let mut state = self.runtime.state.lock().map_err(|_| Error::LockPoisoned)?;
        if state
            .slots
            .get(&self.kind)
            .is_none_or(|s| s.instance != self.instance)
        {
            return Err(Error::Cancelled);
        }
        state.devices = devices.to_vec();
        state.supported = Some(true);
        Ok(())
    }
}
impl Drop for OutputLease {
    fn drop(&mut self) {
        let _ = self.with_slot(|slot| {
            slot.observed.phase = OutputPhase::Dormant;
            slot.observed.driver = None;
            slot.observed.configured = None;
            Ok(())
        });
    }
}
