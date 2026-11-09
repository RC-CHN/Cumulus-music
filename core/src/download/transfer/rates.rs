//! Live body rates are process-local observation, never recovery state.
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

const MIN_SAMPLE: Duration = Duration::from_millis(250);
const WINDOW: Duration = Duration::from_secs(1);
const STALE: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(super) struct LiveRates(Mutex<HashMap<String, Sample>>);

pub(super) struct LiveBody<'a> {
    rates: &'a LiveRates,
    id: &'a str,
}

struct Sample {
    since: Instant,
    bytes: u64,
    last_write: Option<Instant>,
    previous_rate: Option<u64>,
}

impl Sample {
    fn new(now: Instant) -> Self {
        Self {
            since: now,
            bytes: 0,
            last_write: None,
            previous_rate: None,
        }
    }

    fn write(&mut self, now: Instant, bytes: u64) {
        if self.last_write.is_some_and(|last| now - last >= STALE) {
            *self = Self::new(now);
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.last_write = Some(now);
        let elapsed = now - self.since;
        if elapsed >= WINDOW {
            self.previous_rate = Some(bytes_per_second(self.bytes, elapsed));
            self.bytes = 0;
            self.since = now;
        }
    }

    fn rate(&self, now: Instant) -> u64 {
        if self.last_write.is_none_or(|last| now - last >= STALE) {
            return 0;
        }
        self.previous_rate.unwrap_or_else(|| {
            let elapsed = now - self.since;
            if elapsed < MIN_SAMPLE {
                0
            } else {
                bytes_per_second(self.bytes, elapsed)
            }
        })
    }
}

fn bytes_per_second(bytes: u64, elapsed: Duration) -> u64 {
    let scaled = u128::from(bytes) * 1_000_000_000;
    // Keep the optional public integer exact on the renderer's number type.
    (scaled / elapsed.as_nanos()).min((1 << 53) - 1) as u64
}

impl LiveRates {
    pub(super) fn begin<'a>(&'a self, id: &'a str) -> LiveBody<'a> {
        if let Ok(mut rates) = self.0.lock() {
            // The manager's durable per-task lock excludes another live body.
            rates.insert(id.into(), Sample::new(Instant::now()));
        }
        LiveBody { rates: self, id }
    }

    pub(super) fn snapshot(&self) -> HashMap<String, u64> {
        let Ok(rates) = self.0.lock() else {
            return HashMap::new();
        };
        let now = Instant::now();
        rates
            .iter()
            .map(|(id, sample)| (id.clone(), sample.rate(now)))
            .collect()
    }
}

impl LiveBody<'_> {
    pub(super) fn written(&self, bytes: u64) {
        if let Ok(mut rates) = self.rates.0.lock()
            && let Some(sample) = rates.get_mut(self.id)
        {
            sample.write(Instant::now(), bytes);
        }
    }
}

impl Drop for LiveBody<'_> {
    fn drop(&mut self) {
        if let Ok(mut rates) = self.rates.0.lock() {
            rates.remove(self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steady_sample_stalls_and_recovers_without_using_the_old_rate() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut rate = Sample::new(start);
        assert_eq!(rate.rate(at(500)), 0);
        rate.write(at(150), 500);
        assert_eq!(rate.rate(at(200)), 0);
        assert_eq!(rate.rate(at(250)), 2_000);
        rate.write(at(1_000), 2_500);
        assert_eq!(rate.rate(at(1_200)), 3_000);
        assert_eq!(rate.rate(at(3_000)), 0);
        rate.write(at(3_500), 125);
        assert_eq!(rate.rate(at(3_510)), 0);
        assert_eq!(rate.rate(at(3_750)), 500);
    }
}
