use super::*;
use serde_json::json;
impl EventSampler {
    pub fn initialize_battery(
        &mut self,
        sample: BatterySample,
        now_ms: u64,
        random_unit: f64,
    ) -> Result<SampleOutcome> {
        if !self.running {
            return Ok(SampleOutcome::Stopped);
        }
        self.battery_initial = sample.clone();
        self.record_battery(&sample, now_ms, random_unit)
    }
    /// The random input is used only to choose a replacement sample. It never
    /// supplies a battery value. Host callers use operating-system randomness.
    pub fn record_battery(
        &mut self,
        sample: &BatterySample,
        now_ms: u64,
        random_unit: f64,
    ) -> Result<SampleOutcome> {
        Self::time(now_ms)?;
        if !self.running {
            return Ok(SampleOutcome::Stopped);
        }
        if !random_unit.is_finite() || !(0. ..1.).contains(&random_unit) {
            return Err(Error::InvalidInput("invalid Watchman sample randomness"));
        }
        let limit = self.policy.limit(EventKind::Battery);
        let interval = if self.battery.len() < limit.maximum {
            100.
        } else {
            limit.interval_ms
        };
        if (i128::from(now_ms) - i128::from(self.battery_last_ms)) as f64 <= interval {
            return Ok(SampleOutcome::Throttled);
        }
        self.battery_last_ms = now_ms;
        if self.battery.len() >= limit.maximum {
            let victim =
                (values::round(random_unit * (self.battery.len() as f64 - 2.)) + 1.) as usize;
            self.battery.remove(victim);
        }
        self.battery_count = self.battery_count.saturating_add(1);
        let charging = sample.charging.or(self.battery_initial.charging);
        let level = sample
            .level
            .or(self.battery_initial.level)
            .unwrap_or(f64::NAN);
        let time = if charging.unwrap_or(false) {
            sample.charging_time.or(self.battery_initial.charging_time)
        } else {
            sample
                .discharging_time
                .or(self.battery_initial.discharging_time)
        };
        let time = time.filter(|v| v.is_finite()).unwrap_or(-1.);
        self.battery.push(Self::observation(
            &self.descriptors,
            EventKind::Battery,
            json!([
                self.battery_count,
                values::relative(now_ms, self.reference_ms),
                values::flag(charging),
                values::numeric(values::round(level * 100.)),
                values::numeric(time)
            ]),
        )?);
        Ok(SampleOutcome::Stored)
    }
}
