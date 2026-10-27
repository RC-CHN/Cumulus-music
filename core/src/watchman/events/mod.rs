//! Recovered event sampling. Transient per-host state; no keyboard contents,
//! storage, network, desktop framework or synthetic activity generation.
mod battery;
mod model;
mod policy;
mod values;
use super::{Descriptor, Observation, descriptors, encode_field};
use crate::{Error, Result};
pub use model::{
    BatterySample, EventKind, EventSample, Motion, Orientation, PointerPoint, SampleOutcome,
};
pub use policy::{EventPolicy, SamplingLimit};
use rand::seq::SliceRandom;

struct Queue {
    kind: EventKind,
    count: u16,
    last_ms: u64,
    cursor: usize,
    order: Vec<usize>,
    samples: Vec<Observation>,
}
impl Queue {
    fn new(kind: EventKind, maximum: usize) -> Self {
        Self {
            kind,
            count: 0,
            last_ms: 0,
            cursor: 0,
            order: policy::removal_order(maximum),
            samples: vec![],
        }
    }
    fn reset(&mut self) {
        self.count = 0;
        self.last_ms = 0;
        self.cursor = 0;
        self.samples.clear();
    }
}
pub struct EventSampler {
    policy: EventPolicy,
    descriptors: Vec<Descriptor>,
    running: bool,
    reference_ms: u64,
    queues: Vec<Queue>,
    battery: Vec<Observation>,
    battery_initial: BatterySample,
    battery_last_ms: u64,
    battery_count: u16,
}
impl EventSampler {
    pub fn new(policy: EventPolicy) -> Result<Self> {
        Ok(Self {
            descriptors: descriptors()?,
            policy,
            running: false,
            reference_ms: 0,
            queues: vec![],
            battery: vec![],
            battery_initial: BatterySample::default(),
            battery_last_ms: 0,
            battery_count: 0,
        })
    }
    pub fn start(&mut self, now_ms: u64) -> Result<()> {
        Self::time(now_ms)?;
        if !self.running {
            self.reference_ms = now_ms;
            self.running = true;
        }
        Ok(())
    }
    pub fn stop(&mut self) {
        self.running = false;
        self.clear();
        self.battery_initial = BatterySample::default();
    }
    pub fn clear(&mut self) {
        for q in &mut self.queues {
            q.reset();
        }
        self.battery.clear();
        self.battery_count = 0;
        self.battery_initial = BatterySample::default();
    }
    pub fn set_reference(&mut self, now_ms: u64) -> Result<()> {
        Self::time(now_ms)?;
        self.reference_ms = now_ms;
        Ok(())
    }
    fn time(now: u64) -> Result<()> {
        if now > 8_640_000_000_000_000 {
            Err(Error::InvalidInput("Watchman event time outside JS range"))
        } else {
            Ok(())
        }
    }
    fn observation(
        table: &[Descriptor],
        kind: EventKind,
        value: serde_json::Value,
    ) -> Result<Observation> {
        let descriptor = table
            .iter()
            .find(|f| f.key == kind.key())
            .ok_or(Error::State("missing Watchman event descriptor"))?;
        encode_field(descriptor, &value, false)?; // encoding failure occurs at capture, as in V.O
        Ok(Observation {
            key: kind.key().into(),
            value,
            failed: false,
        })
    }
    pub fn record(&mut self, event: &EventSample, now_ms: u64) -> Result<SampleOutcome> {
        Self::time(now_ms)?;
        if !self.running {
            return Ok(SampleOutcome::Stopped);
        }
        if event.target_id.len() > 4096 {
            return Err(Error::InvalidInput("Watchman target ID exceeds limit"));
        }
        let kind = EventKind::from_dom(&event.dom_type)
            .ok_or(Error::InvalidInput("unsupported Watchman DOM event"))?;
        let limit = self.policy.limit(kind);
        let index = self
            .queues
            .iter()
            .position(|q| q.kind == kind)
            .unwrap_or_else(|| {
                self.queues.push(Queue::new(kind, limit.maximum));
                self.queues.len() - 1
            });
        let q = &mut self.queues[index];
        let interval = if q.samples.len() < limit.maximum {
            10.
        } else {
            limit.interval_ms
        };
        if (i128::from(now_ms) - i128::from(q.last_ms)) as f64 <= interval {
            return Ok(SampleOutcome::Throttled);
        }
        q.last_ms = now_ms;
        if q.samples.len() >= limit.maximum {
            let victim = q.order[q.cursor];
            q.cursor = (q.cursor + 1) % q.order.len();
            q.samples.remove(victim);
        }
        q.count = q.count.saturating_add(1);
        let Some(value) = values::values(kind, event, q.count, now_ms, self.reference_ms)? else {
            return Ok(SampleOutcome::MissingMeasurement);
        };
        q.samples
            .push(Self::observation(&self.descriptors, kind, value)?);
        Ok(SampleOutcome::Stored)
    }
    /// V.J shuffles groups and each group's samples; battery retains its order
    /// and is appended. A drain resets DOM counters/throttle/removal cursors,
    /// while battery's throttle clock persists, matching n.J. V.A resets the
    /// shared counters, including the battery counter, before n.J drains it.
    pub fn drain_with_rng<R: rand::Rng + ?Sized>(&mut self, rng: &mut R) -> Vec<Observation> {
        let mut groups: Vec<_> = (0..self.queues.len()).collect();
        groups.shuffle(rng);
        let mut out = vec![];
        for i in groups {
            let q = &mut self.queues[i];
            q.samples.shuffle(rng);
            out.append(&mut q.samples);
            q.reset();
        }
        // V.A resets the shared pa object, including battery's sequence counter.
        self.battery_count = 0;
        out.append(&mut self.battery);
        out
    }
    pub fn drain(&mut self) -> Vec<Observation> {
        self.drain_with_rng(&mut rand::rng())
    }
    pub fn buffered(&self) -> usize {
        self.battery.len() + self.queues.iter().map(|q| q.samples.len()).sum::<usize>()
    }
}
