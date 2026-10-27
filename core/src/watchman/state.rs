use super::{Config, StoredValue, encode_nd_info};
use crate::{Error, Result, protocol::wire::serialization::js_truthy, storage::StateStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

const OWNER: &str = "device";
const KIND: &str = "watchman-device-v1";

/// Private native state: no Debug and not an IPC DTO. SQLite replaces the SDK's
/// browser storage fallback chain; product namespaces never contain music cookies.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceState {
    revision: u64,
    did: String,
    version: String,
    created_ms: u64,
    expires_ms: u64,
    tid: String,
    ni: String,
    clock_offset_ms: i64,
}
impl DeviceState {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn did_native(&self) -> &str {
        &self.did
    }
    pub fn tid_native(&self) -> &str {
        &self.tid
    }
    pub fn ni_native(&self) -> &str {
        &self.ni
    }
    pub fn clock_offset_ms(&self) -> i64 {
        self.clock_offset_ms
    }
    pub fn expires_ms(&self) -> u64 {
        self.expires_ms
    }
    pub fn ready(&self, config: &Config, now_ms: u64) -> bool {
        if !config.device_enabled {
            return true;
        }
        self.version_compatible(config)
            && now_ms > self.created_ms
            && now_ms <= self.expires_ms
            && !self.did.is_empty()
            && !self.tid.is_empty()
            && !self.ni.is_empty()
    }
    pub(crate) fn version_compatible(&self, config: &Config) -> bool {
        config.last_used_version.is_empty()
            || self.version.is_empty()
            || self.version == config.build_version()
            || self.version == config.last_used_version
    }
    /// Caller schedules the official five-minute poll; this is its decision.
    pub fn refresh_due(&self, config: &Config, now_ms: u64) -> bool {
        config.device_enabled && self.expires_ms.saturating_sub(now_ms) <= 300_000
    }
    pub fn nd_info(&self, config: &Config) -> Result<String> {
        encode_nd_info(&config.ret, &self.did, StoredValue::Text(&self.ni))
    }
    fn validate(&self) -> Result<()> {
        for value in [&self.did, &self.tid, &self.ni, &self.version] {
            if value.len() > 4096 || value.contains(['\r', '\n', '\0']) {
                return Err(Error::State(
                    "corrupt Watchman identifiers; replacement refused",
                ));
            }
        }
        if self.expires_ms < self.created_ms {
            return Err(Error::State("corrupt Watchman expiry"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKind {
    Device,
    Behavior,
}
#[derive(Debug, PartialEq, Eq)]
pub enum ReportUpdate {
    Accepted,
    ClockCorrection,
    IllegalBusiness,
}

pub struct DeviceStore {
    store: Arc<StateStore>,
    config: Config,
}
impl DeviceStore {
    pub fn new(store: Arc<StateStore>, config: Config) -> Self {
        Self { store, config }
    }
    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn snapshot(&self) -> Result<DeviceState> {
        let state = decode(self.store.get(OWNER, KIND, &self.config.product)?)?;
        state.validate()?;
        Ok(state)
    }
    /// Apply the reviewed response-array slots atomically. `server_time_ms` is
    /// supplied by the native transport after parsing slot 1 as a JS Date. It is
    /// required only for truthy time on 420; it is not used by request_nonce.
    pub fn apply_response(
        &self,
        expected_revision: u64,
        kind: ReportKind,
        response: &Value,
        now_ms: u64,
        server_time_ms: Option<i64>,
    ) -> Result<ReportUpdate> {
        let fields = response
            .as_array()
            .ok_or(Error::Response("Watchman response is not an array"))?;
        let code = fields
            .first()
            .and_then(Value::as_i64)
            .ok_or(Error::Response("Watchman response code is not numeric"))?;
        if code == 470 && kind == ReportKind::Behavior {
            return Ok(ReportUpdate::IllegalBusiness);
        }
        if code != 200 && code != 420 {
            return Err(Error::Response("Watchman report rejected"));
        }
        let identifier = |index: usize| -> Result<Option<&str>> {
            match fields.get(index).filter(|v| js_truthy(v)) {
                None => Ok(None),
                Some(v) => v
                    .as_str()
                    .filter(|s| s.len() <= 4096 && !s.contains(['\r', '\n', '\0']))
                    .map(Some)
                    .ok_or(Error::Response("invalid Watchman response identifier")),
            }
        };
        let tid = identifier(2)?;
        let did = if kind == ReportKind::Device {
            identifier(3)?
        } else {
            None
        };
        let ni = if kind == ReportKind::Device {
            identifier(5)?
        } else {
            None
        };
        let offset = if code == 420 && fields.get(1).is_some_and(js_truthy) {
            Some(
                server_time_ms
                    .ok_or(Error::Response("Watchman correction time was not parsed"))?
                    .checked_sub(
                        i64::try_from(now_ms)
                            .map_err(|_| Error::InvalidInput("Watchman clock exceeds range"))?,
                    )
                    .ok_or(Error::Response("Watchman clock offset overflow"))?,
            )
        } else {
            None
        };
        self.store.transaction(|tx| {
            let mut state = decode(tx.get(OWNER, KIND, &self.config.product)?)?;
            state.validate()?;
            if state.revision != expected_revision {
                return Err(Error::StaleResponse);
            }
            if let Some(did) = did {
                state.did = did.into();
                state.version = self.config.build_version().into();
                state.created_ms = now_ms;
                let ttl = (u128::from(self.config.device_ttl_ms) * 5 / 6) as u64;
                state.expires_ms = now_ms
                    .checked_add(ttl)
                    .ok_or(Error::InvalidInput("Watchman expiry overflow"))?;
            }
            if let Some(tid) = tid {
                state.tid = tid.into();
            }
            if let Some(ni) = ni {
                state.ni = ni.into();
            }
            if let Some(offset) = offset {
                state.clock_offset_ms = offset;
            }
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or(Error::State("Watchman revision exhausted"))?;
            tx.put(
                OWNER,
                KIND,
                &self.config.product,
                &serde_json::to_value(state)?,
            )
        })?;
        Ok(if code == 420 {
            ReportUpdate::ClockCorrection
        } else {
            ReportUpdate::Accepted
        })
    }
    /// Explicit invalidation advances the revision so an in-flight response
    /// cannot restore discarded identifiers after reset or a transport change.
    pub fn invalidate(&self, expected_revision: u64) -> Result<()> {
        self.store.transaction(|tx| {
            let old = decode(tx.get(OWNER, KIND, &self.config.product)?)?;
            if old.revision != expected_revision {
                return Err(Error::StaleResponse);
            }
            let revision = old
                .revision
                .checked_add(1)
                .ok_or(Error::State("Watchman revision exhausted"))?;
            tx.put(
                OWNER,
                KIND,
                &self.config.product,
                &serde_json::to_value(DeviceState {
                    revision,
                    ..Default::default()
                })?,
            )
        })
    }
}
fn decode(value: Option<Value>) -> Result<DeviceState> {
    value
        .map(serde_json::from_value)
        .transpose()
        .map(|v| v.unwrap_or_default())
        .map_err(Error::from)
}
