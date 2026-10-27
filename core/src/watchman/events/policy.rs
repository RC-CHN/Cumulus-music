use super::super::value::js_number;
use super::EventKind;
use serde_json::Value;

#[derive(Clone, Copy)]
pub struct SamplingLimit {
    pub maximum: usize,
    pub interval_ms: f64,
}
#[derive(Clone)]
pub struct EventPolicy {
    limits: [SamplingLimit; 11],
}
impl EventPolicy {
    /// h()/k(): server maxima can only reduce the defaults; intervals can only
    /// increase them. Fractional positive maxima imply ceil(max) stored items.
    pub fn from_options(options: &Value, xhr_cors_supported: bool) -> Self {
        let specs = [
            ("mem", "mei", 100, 3, 300.),
            ("cem", "cei", 20, 3, 300.),
            ("cem", "cei", 20, 3, 300.),
            ("cem", "cei", 20, 1, 300.),
            ("kem", "kei", 10, 2, 300.),
            ("fem", "fei", 20, 1, 300.),
            ("fem", "fei", 20, 1, 300.),
            ("sem", "sei", 20, 2, 300.),
            ("otem", "otei", 50, 2, 3000.),
            ("mtem", "mtei", 50, 2, 3000.),
            ("", "", 100, 3, 10.),
        ];
        let limits = specs.map(|(max, interval, large, small, default_interval)| {
            let cap = if xhr_cors_supported { large } else { small };
            let positive = |key: &str| {
                if key.is_empty() {
                    return None;
                }
                options.get(key).map(js_number).filter(|n| *n > 0.)
            };
            SamplingLimit {
                maximum: positive(max).map_or(cap, |n| n.min(cap as f64).ceil() as usize),
                interval_ms: positive(interval)
                    .map_or(default_interval, |n| n.max(default_interval)),
            }
        });
        Self { limits }
    }
    pub fn limit(&self, kind: EventKind) -> SamplingLimit {
        self.limits[kind as usize]
    }
}
/// jd(): repeated in-order traversal of the implicit complete binary tree.
pub(super) fn removal_order(maximum: usize) -> Vec<usize> {
    fn visit(index: usize, maximum: usize, out: &mut Vec<usize>) {
        if index < maximum {
            visit(index * 2 + 1, maximum, out);
            out.push(index);
            visit(index * 2 + 2, maximum, out);
        }
    }
    let mut out = Vec::with_capacity(maximum);
    visit(0, maximum, &mut out);
    out
}
