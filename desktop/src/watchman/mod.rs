//! Native event bridge. Activation requires native reviewed configuration;
//! no token or music credential is exposed to the WebView. Not auto-enabled.
pub mod collection;
pub mod compatibility;
pub mod host;
pub mod measurement;
use cumulus_core::{
    Error, Result,
    watchman::{Config, EntropyClock, Observation, SystemEntropyClock, events::*},
};
use serde::Deserialize;
use std::sync::Mutex;
use tauri::Manager;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EventPayload {
    Event {
        sample: Box<EventSample>,
    },
    Battery {
        sample: BatterySample,
        initial: bool,
    },
    Overflow,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventFrame {
    generation: u64,
    sequence: u64,
    observed_ms: u64,
    payload: EventPayload,
}
struct Active {
    generation: u64,
    sequence: u64,
    sampler: EventSampler,
    faulted: bool,
}
#[derive(Default)]
struct State {
    generation: u64,
    active: Option<Active>,
}
#[derive(Default)]
pub struct EventHost(Mutex<State>);
impl EventHost {
    pub fn begin(&self, config: &Config, cors_supported: bool, now_ms: u64) -> Result<u64> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let mut sampler = EventSampler::new(EventPolicy::from_options(
            config.collection_options(),
            cors_supported,
        ))?;
        sampler.start(now_ms)?;
        state.generation = state
            .generation
            .checked_add(1)
            .filter(|g| *g <= 9_007_199_254_740_991)
            .ok_or(Error::State("Watchman host generation exhausted"))?;
        let generation = state.generation;
        state.active = Some(Active {
            generation,
            sequence: 0,
            sampler,
            faulted: false,
        });
        Ok(generation)
    }
    pub fn stop(&self) -> Result<()> {
        self.0.lock().map_err(|_| Error::LockPoisoned)?.active = None;
        Ok(())
    }
    pub fn receive(&self, frame: EventFrame) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let active = state
            .active
            .as_mut()
            .filter(|s| s.generation == frame.generation)
            .ok_or(Error::StaleResponse)?;
        if active.faulted {
            return Err(Error::State("Watchman event bridge requires restart"));
        }
        if active.sequence.checked_add(1) != Some(frame.sequence) {
            active.faulted = true;
            return Err(Error::State("Watchman event sequence gap"));
        }
        active.sequence = frame.sequence;
        let result = match frame.payload {
            EventPayload::Event { sample } => active
                .sampler
                .record(&sample, frame.observed_ms)
                .map(|_| ()),
            EventPayload::Battery { sample, initial } => (|| {
                let mut bytes = [0; 8];
                SystemEntropyClock.fill(&mut bytes)?;
                let random = (u64::from_be_bytes(bytes) >> 11) as f64 / 9_007_199_254_740_992.;
                if initial {
                    active
                        .sampler
                        .initialize_battery(sample, frame.observed_ms, random)
                } else {
                    active
                        .sampler
                        .record_battery(&sample, frame.observed_ms, random)
                }
                .map(|_| ())
            })(),
            EventPayload::Overflow => Err(Error::State("Watchman browser event backlog overflow")),
        };
        if result.is_err() {
            active.faulted = true;
        }
        result
    }
    /// Called by the native behavior collection, never exposed as an IPC.
    pub fn drain(&self, generation: u64, reference_ms: u64) -> Result<Vec<Observation>> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let active = state
            .active
            .as_mut()
            .filter(|s| s.generation == generation)
            .ok_or(Error::StaleResponse)?;
        if active.faulted {
            return Err(Error::State("Watchman event bridge requires restart"));
        }
        active.sampler.set_reference(reference_ms)?;
        Ok(active.sampler.drain())
    }
    pub fn received(&self, generation: u64) -> Result<u64> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let active = state
            .active
            .as_ref()
            .filter(|s| s.generation == generation)
            .ok_or(Error::StaleResponse)?;
        if active.faulted {
            return Err(Error::State("Watchman event bridge requires restart"));
        }
        Ok(active.sequence)
    }
}
pub fn start_events<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    config: &Config,
    cors_supported: bool,
) -> Result<u64> {
    if window.label() != "main" {
        return Err(Error::InvalidInput("Watchman events require main window"));
    }
    let host = window.state::<EventHost>();
    let generation = host.begin(config, cors_supported, SystemEntropyClock.now_ms()?)?;
    if window
        .eval(format!("({})({generation});", include_str!("events.js")))
        .is_err()
    {
        host.stop()?;
        return Err(Error::State("Watchman event observer installation failed"));
    }
    Ok(generation)
}
pub fn stop_events<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> Result<()> {
    window.state::<EventHost>().stop()?;
    window
        .eval("window.__cumulusWatchmanEvents?.stop();")
        .map_err(|_| Error::State("Watchman event observer detach failed"))
}
#[tauri::command]
pub(crate) fn watchman_observation<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    host: tauri::State<'_, EventHost>,
    frame: EventFrame,
) -> std::result::Result<(), String> {
    if window.label() != "main" {
        return Err("watchman_observation_rejected".into());
    }
    host.receive(frame)
        .map_err(|_| "watchman_observation_rejected".into())
}
