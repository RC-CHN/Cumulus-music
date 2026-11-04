use super::{OfficialPlayingState, PlaybackKeepAwake, PlaybackPowerRequest};
use crate::{Error, Result, storage::StateStore};
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

const OWNER: &str = "local-device";
const KIND: &str = "playback-power-v1";
const KEY: &str = "enabled";

/// The desktop implementation must submit without waiting for the operating system.
pub trait PlaybackPowerSink: Send + Sync {
    fn submit(&self, request: PlaybackPowerRequest);
    fn available(&self) -> bool {
        true
    }
}

pub(crate) struct PowerControl {
    pub sink: Arc<dyn PlaybackPowerSink>,
    enabled: AtomicBool,
    write: Mutex<()>,
}
impl PowerControl {
    pub(crate) fn new(store: &StateStore, sink: Arc<dyn PlaybackPowerSink>) -> Self {
        // Missing or damaged records do not keep the device awake.
        let enabled = matches!(store.get(OWNER, KIND, KEY), Ok(Some(Value::Bool(true))));
        Self {
            sink,
            enabled: AtomicBool::new(enabled),
            write: Mutex::new(()),
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub(crate) fn set(&self, store: &StateStore, enabled: bool) -> Result<()> {
        let _write = self.write.lock().map_err(|_| Error::LockPoisoned)?;
        store.put(OWNER, KIND, KEY, &Value::Bool(enabled))?;
        self.enabled.store(enabled, Ordering::Release);
        Ok(())
    }
}

pub(crate) struct PowerEffect {
    control: Arc<PowerControl>,
    policy: PlaybackKeepAwake,
    intent: OfficialPlayingState,
}
impl PowerEffect {
    pub(crate) fn new(control: Arc<PowerControl>) -> Self {
        Self {
            control,
            policy: Default::default(),
            intent: OfficialPlayingState::Stop,
        }
    }

    pub(crate) fn set(&mut self, intent: OfficialPlayingState) {
        self.intent = intent;
        self.observe();
    }

    pub(crate) fn observe(&mut self) {
        if let Some(request) = self.policy.observe(self.control.enabled(), self.intent) {
            self.control.sink.submit(request);
        }
    }
}

impl Drop for PowerEffect {
    fn drop(&mut self) {
        self.set(OfficialPlayingState::Stop);
    }
}
