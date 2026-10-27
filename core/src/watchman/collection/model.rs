use super::*;
use serde_json::Value;

#[derive(Clone, Copy)]
pub enum TimingSlot {
    Initialization = 0,
    Packet = 1,
    Configuration = 2,
    Device = 3,
    Events = 4,
    Automation = 5,
}

/// Deliberately no Debug: raw browser text/fingerprint preimages are private.
pub enum MeasurementReading {
    Value(Value),
    Fingerprint(FingerprintReading),
    Failed,
}
pub struct EnvironmentReading {
    pub reading: MeasurementReading,
    pub automation_ms: Option<i64>,
}
/// A host instance owns one immutable identity declaration. Replacing a page or
/// its declared identity must change epoch; stale observations cannot populate
/// a new page's cache. Errors mean host/transport failure, Failed means an actual
/// getter/serialization failure and follows the SDK's field/group rules.
pub trait MeasurementSource: Send + Sync {
    fn epoch(&self) -> Result<u64>;
    fn initialize<'a>(&'a self, build: SdkBuild, config: &'a Config) -> ApiFuture<'a, ()>;
    fn device<'a>(&'a self, key: &'a str, deferred: bool) -> ApiFuture<'a, MeasurementReading>;
    fn environment<'a>(
        &'a self,
        key: &'a str,
        timing: [i64; 6],
        elapsed_ms: u64,
    ) -> ApiFuture<'a, EnvironmentReading>;
    /// Must await all preceding browser event IPCs before native draining.
    fn drain_events<'a>(
        &'a self,
        config: &'a Config,
        reference_ms: u64,
    ) -> ApiFuture<'a, Vec<Observation>>;
}
pub(super) fn observation(key: &str, reading: MeasurementReading) -> Result<Observation> {
    match reading {
        MeasurementReading::Fingerprint(reading) => fingerprint_observation(key, reading),
        MeasurementReading::Failed => Ok(Observation {
            key: key.into(),
            value: Value::Null,
            failed: true,
        }),
        MeasurementReading::Value(value) => {
            if serde_json::to_vec(&value)?.len() > MAX_BYTES {
                return Err(Error::InvalidInput("measurement exceeds maximum size"));
            }
            Ok(Observation {
                key: key.into(),
                value,
                failed: false,
            })
        }
    }
}
pub(super) fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}
