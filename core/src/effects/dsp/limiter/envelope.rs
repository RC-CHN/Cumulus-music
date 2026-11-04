use super::{Format, LimiterSettings};
use std::collections::VecDeque;

#[derive(Clone, Copy)]
struct PeakPoint {
    slot: usize,
    slope: f64,
}

pub(super) struct Envelope {
    threshold: f64,
    pre_gain: f64,
    post_gain: f64,
    release_duration: f64,
    adaptive_factor: f64,
    infinite_ratio: bool,
    channels: usize,
    window: usize,
    ring: Vec<f64>,
    slot: usize,
    gain: f64,
    slope: f64,
    peaks: VecDeque<PeakPoint>,
    peak_count: u32,
    peak_sum: f64,
}

fn db_gain(db: f32) -> f64 {
    // 71DE0 divides in f32, calls pow(10, double(exponent)), and returns f32.
    f64::from(10.0_f64.powf(f64::from(db / 20.0)) as f32)
}

impl Envelope {
    pub(super) fn new(format: Format, window: usize, settings: LimiterSettings) -> Self {
        let channels = format.channels();
        Self {
            threshold: db_gain(settings.threshold_db),
            pre_gain: db_gain(settings.input_gain_db),
            post_gain: db_gain(settings.output_gain_db),
            release_duration: f64::from(format.rate()) * f64::from(settings.release_seconds),
            adaptive_factor: -(0.5_f64.powf(1.0 / f64::from(settings.ratio) - 0.5) * 2.0),
            infinite_ratio: settings.infinite_ratio,
            channels,
            window,
            ring: vec![0.0; channels * window],
            slot: 0,
            gain: 1.0,
            slope: 0.0,
            peaks: VecDeque::with_capacity(window),
            peak_count: 0,
            peak_sum: 0.0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.ring.fill(0.0);
        self.slot = 0;
        self.gain = 1.0;
        self.slope = 0.0;
        self.peaks.clear();
        self.peak_count = 0;
        self.peak_sum = 0.0;
    }

    fn linked_peak(&self, slot: usize) -> f64 {
        let first = slot * self.channels;
        self.ring[first..first + self.channels]
            .iter()
            .fold(0.0_f64, |peak, sample| peak.max(sample.abs()))
    }

    fn distance(&self, end: usize, start: usize) -> f64 {
        ((end + self.window - start) % self.window) as f64
    }

    fn release_slope(&self, gain: f64, adaptive: bool) -> f64 {
        let baseline = (1.0 - gain) / self.release_duration;
        if adaptive && self.infinite_ratio && self.peak_count > 0 {
            // 63E10 tests this signed candidate before applying its lower and
            // upper slope bounds. Keep the native sign and evaluation order.
            let candidate = self.threshold / (self.adaptive_factor * self.peak_sum)
                * f64::from(self.peak_count);
            if candidate > gain {
                return ((candidate - gain) / self.release_duration)
                    .max(baseline / 10.0)
                    .min(baseline);
            }
        }
        baseline
    }

    fn observe_peak(&mut self, peak: f64) {
        if peak <= self.threshold {
            return;
        }
        if self.infinite_ratio {
            self.peak_count += 1;
            self.peak_sum += peak;
        }
        let target = self.threshold / peak;
        let release = self.release_slope(target.min(1.0), false);
        // 641E3–64218 divides by the ring size in samples, then multiplies
        // by channels. Using a frame denominator would change rounding.
        let incoming = (target - self.gain) / self.ring.len() as f64 * self.channels as f64;
        let point = PeakPoint {
            slot: self.slot,
            slope: release,
        };
        if self.slope > incoming {
            self.slope = incoming;
            self.peaks.clear();
            self.peaks.push_back(point);
            return;
        }
        // 64290–64422: an intersection revises the first steeper prior
        // point, drops the now-hidden suffix and schedules this new peak.
        for (index, previous) in self.peaks.iter().copied().enumerate() {
            let previous_target = self.threshold / self.linked_peak(previous.slot);
            let crossing = (target - previous_target) / self.distance(self.slot, previous.slot);
            if previous.slope > crossing {
                self.peaks[index].slope = crossing;
                self.peaks.truncate(index + 1);
                self.peaks.push_back(point);
                return;
            }
        }
    }

    fn retire_peak(&mut self, read: usize, peak: f64) {
        if let Some(point) = self
            .peaks
            .front()
            .copied()
            .filter(|point| point.slot == read)
        {
            if self.infinite_ratio {
                self.slope = self.release_slope(self.gain, true);
                if let Some(next) = self.peaks.get(1) {
                    let next_slope = (self.threshold / self.linked_peak(next.slot) - self.gain)
                        / self.distance(next.slot, read);
                    if self.slope > next_slope {
                        self.slope = next_slope;
                    }
                }
            } else {
                self.slope = point.slope;
                self.gain = self.threshold / peak;
            }
            self.peaks.pop_front();
        }
    }

    fn settle(&mut self) {
        if self.gain > 1.0 {
            self.gain = 1.0;
            self.slope = 0.0;
            self.peaks.clear();
        }
        if self.gain <= 0.0 {
            self.gain = 1e-13;
            self.slope = 0.999_999_999_999_9 / self.release_duration;
        }
        if self.gain != 1.0 && 1.0 - self.gain < 1e-13 {
            self.gain = 1.0;
        }
        if self.slope != 0.0 && self.slope.abs() < 1e-14 {
            self.slope = 0.0;
        }
    }

    pub(super) fn step(&mut self, input: &[f32], output: &mut [f64]) -> (f64, f64) {
        let write = self.slot * self.channels;
        for (sample, value) in self.ring[write..write + self.channels]
            .iter_mut()
            .zip(input)
        {
            *sample = self.pre_gain * f64::from(*value);
        }
        let peak = self.linked_peak(self.slot);
        self.observe_peak(peak);

        let read = (self.slot + 1) % self.window;
        let delayed_peak = self.linked_peak(read);
        if self.infinite_ratio && delayed_peak > self.threshold {
            self.peak_count -= 1;
            self.peak_sum -= delayed_peak;
        }
        self.gain += self.slope;
        let applied_gain = self.gain;
        let start = read * self.channels;
        for (out, delayed) in output
            .iter_mut()
            .zip(&self.ring[start..start + self.channels])
        {
            *out = *delayed * applied_gain;
        }
        self.retire_peak(read, delayed_peak);
        self.settle();
        // 64902–64ACB clips before applying output gain. Native +5C can add
        // threshold normalization, but is zero on the tested material path
        // and is not the +58 infinite-ratio switch.
        for value in output {
            *value = value.clamp(-self.threshold, self.threshold) * self.post_gain;
        }
        self.slot = read;
        (peak, applied_gain)
    }

    #[cfg(test)]
    pub(super) fn state(&self) -> (f64, f64, usize, u32, f64) {
        (
            self.gain,
            self.slope,
            self.peaks.len(),
            self.peak_count,
            self.peak_sum,
        )
    }
}
