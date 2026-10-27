//! Native collection order, configuration gates, caching and abort semantics.
//! Browser facts and event flushing are supplied through an explicit host.
mod model;
mod policy;
use super::*;
use crate::{Error, Result, protocol::api::ApiFuture};
pub use model::{EnvironmentReading, MeasurementReading, MeasurementSource, TimingSlot};
use rand::seq::SliceRandom;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::Mutex as AsyncMutex;

#[derive(Default)]
struct State {
    epoch: Option<u64>,
    build: Option<SdkBuild>,
    device: Option<Vec<Observation>>,
    environment_expanded: bool,
}
pub struct NativeCollection {
    source: Arc<dyn MeasurementSource>,
    state: AsyncMutex<State>,
    timings: Mutex<[i64; 6]>,
}
impl NativeCollection {
    pub fn new(source: Arc<dyn MeasurementSource>) -> Self {
        Self {
            source,
            state: AsyncMutex::new(State::default()),
            timings: Mutex::new([0; 6]),
        }
    }
    fn check_epoch(&self, epoch: u64) -> Result<()> {
        if self.source.epoch()? != epoch {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
    async fn run(
        &self,
        mode: CollectionMode,
        config: &Config,
        elapsed: u64,
        reference: u64,
    ) -> Result<Observations> {
        let mut state = self.state.lock().await;
        let epoch = self.source.epoch()?;
        if state.epoch != Some(epoch) || state.build != Some(config.build()) {
            *state = State::default();
            let started = Instant::now();
            self.source.initialize(config.build(), config).await?;
            self.check_epoch(epoch)?;
            {
                let mut timings = self.timings.lock().map_err(|_| Error::LockPoisoned)?;
                for slot in [0, 1, 3, 4, 5] {
                    timings[slot] = 0;
                }
            }
            self.record_timing(TimingSlot::Initialization, model::elapsed_ms(started))?;
            state.epoch = Some(epoch);
            state.build = Some(config.build());
        }
        let fields = descriptors()?;
        let mut result = Observations::default();
        if mode == CollectionMode::Device {
            let started = Instant::now();
            if state.device.is_none() {
                let mut facts = Vec::new();
                // Official ordinary getters precede deferred getters. The host
                // yields before each deferred browser operation; callback audio
                // resolves before the complete device cache is published.
                for item in policy::device_plan(config)? {
                    let reading = self.source.device(&item.key, item.deferred).await?;
                    self.check_epoch(epoch)?;
                    let fact = model::observation(&item.key, reading)?;
                    let field = fields
                        .iter()
                        .find(|f| f.key == item.key)
                        .ok_or(Error::State("missing device descriptor"))?;
                    facts.push(if encode_field(field, &fact.value, fact.failed).is_ok() {
                        fact
                    } else {
                        Observation {
                            key: item.key,
                            value: serde_json::Value::Null,
                            failed: true,
                        }
                    });
                }
                // Ec shuffles once before caching its encoded device group.
                facts.shuffle(&mut rand::rng());
                state.device = Some(facts);
            }
            result.device = state
                .device
                .clone()
                .ok_or(Error::State("device cache unavailable"))?;
            self.record_timing(TimingSlot::Device, model::elapsed_ms(started))?;
        } else {
            let started = Instant::now();
            // Official behavior drains events before measuring environment, so
            // the current drain duration is visible in the lb timing field.
            result.events = self.source.drain_events(config, reference).await?;
            self.check_epoch(epoch)?;
            self.record_timing(TimingSlot::Events, model::elapsed_ms(started))?;
            // ca(false) mutates Cb with Dc. Subsequent ca(true) also sees all 21.
            state.environment_expanded = true;
        }
        let mut keys = policy::environment_keys(state.environment_expanded)?;
        keys.shuffle(&mut rand::rng());
        for key in keys {
            let timing = *self.timings.lock().map_err(|_| Error::LockPoisoned)?;
            let observed = self.source.environment(&key, timing, elapsed).await?;
            self.check_epoch(epoch)?;
            if let Some(duration) = observed.automation_ms {
                if duration.unsigned_abs() > 9_007_199_254_740_991 {
                    return Err(Error::InvalidInput("invalid browser collection duration"));
                }
                self.timings.lock().map_err(|_| Error::LockPoisoned)?
                    [TimingSlot::Automation as usize] = duration;
            }
            let fact = model::observation(&key, observed.reading)?;
            let field = fields
                .iter()
                .find(|f| f.key == key)
                .ok_or(Error::State("missing environment descriptor"))?;
            // Official ca wraps the whole traversal in one try/catch. Do not
            // turn an exception into a failed field and continue the group.
            if fact.failed || encode_field(field, &fact.value, false).is_err() {
                break;
            }
            result.environment.push(fact);
        }
        self.check_epoch(epoch)?;
        Ok(result)
    }
}
impl Collection for NativeCollection {
    fn collect<'a>(
        &'a self,
        mode: CollectionMode,
        config: &'a Config,
        elapsed_ms: u64,
    ) -> ApiFuture<'a, Observations> {
        Box::pin(async move {
            self.run(mode, config, elapsed_ms, SystemEntropyClock.now_ms()?)
                .await
        })
    }
    fn collect_at<'a>(
        &'a self,
        mode: CollectionMode,
        config: &'a Config,
        elapsed_ms: u64,
        reference_ms: u64,
    ) -> ApiFuture<'a, Observations> {
        Box::pin(self.run(mode, config, elapsed_ms, reference_ms))
    }
    fn record_timing(&self, slot: TimingSlot, elapsed_ms: u64) -> Result<()> {
        if elapsed_ms > 9_007_199_254_740_991 {
            return Err(Error::InvalidInput("invalid collection duration"));
        }
        self.timings.lock().map_err(|_| Error::LockPoisoned)?[slot as usize] = elapsed_ms as i64;
        Ok(())
    }
    fn preserves_field_order(&self) -> bool {
        true
    }
}
