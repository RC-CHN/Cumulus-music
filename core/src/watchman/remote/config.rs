use super::{ApiHost, Request, RequestKind, Transport};
use crate::{
    Error, Result,
    storage::StateStore,
    watchman::{Config, EntropyClock, SdkBuild, sdk_uuid},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::Mutex;
const OWNER: &str = "device";
const KIND: &str = "watchman-config-v1";

#[derive(Clone)]
pub struct ConfigSnapshot {
    pub(crate) config: Config,
    pub(crate) hosts: Vec<ApiHost>,
    cache_key: String,
    interval_ms: u64,
    expires_ms: u64,
    revision: u64,
    raw: Value,
}
impl ConfigSnapshot {
    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn hosts(&self) -> &[ApiHost] {
        &self.hosts
    }
    pub fn expires_ms(&self) -> u64 {
        self.expires_ms
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn response_native(&self) -> &Value {
        &self.raw
    }
    fn parse(product: &str, raw: Value, expires_ms: u64, revision: u64) -> Result<Self> {
        if raw["code"].as_i64() != Some(200) {
            return Err(Error::Response("Watchman configuration rejected"));
        }
        let data = &raw["result"];
        let build = SdkBuild::parse(data["v"].as_str().unwrap_or(""))?;
        let conf = data["conf"]
            .as_str()
            .ok_or(Error::Response("missing Watchman conf"))?;
        let last = data["luv"].as_str().unwrap_or("");
        let config = Config::from_conf_for(build, product, last, conf)?;
        let interval_ms = data["ivp"]
            .as_u64()
            .filter(|n| *n <= 9_007_199_254_740_991)
            .ok_or(Error::Response("invalid Watchman config expiry"))?;
        let cache_key = data["cvk"].as_str().unwrap_or("").to_owned();
        if cache_key.len() > 256 || cache_key.contains(['\r', '\n', '\0']) {
            return Err(Error::Response("invalid Watchman config cache key"));
        }
        let raw_hosts = data
            .get("ass")
            .filter(|v| !v.is_null())
            .unwrap_or(&data["as"]);
        let mut hosts = if let Some(values) = raw_hosts.as_array() {
            values
                .iter()
                .map(|v| {
                    v.as_str()
                        .ok_or(Error::Response("invalid Watchman hosts"))
                        .and_then(ApiHost::parse)
                })
                .collect::<Result<Vec<_>>>()?
        } else if let Some(value) = raw_hosts.as_str() {
            value
                .split("::")
                .map(ApiHost::parse)
                .collect::<Result<Vec<_>>>()?
        } else {
            return Err(Error::Response("missing Watchman report hosts"));
        };
        if hosts.is_empty() || hosts.len() > 8 {
            return Err(Error::Response("invalid Watchman host count"));
        }
        if hosts.len() == 1 {
            hosts.push(hosts[0]);
        } // recovered loader duplicates a single host
        Ok(Self {
            config,
            hosts,
            cache_key,
            interval_ms,
            expires_ms,
            revision,
            raw,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    revision: u64,
    expires_ms: u64,
    raw: Value,
}

pub struct ConfigService {
    store: Arc<StateStore>,
    backend: Arc<dyn Transport>,
    clock: Arc<dyn EntropyClock>,
    product: String,
    gate: Mutex<()>,
    callbacks: AtomicU64,
}
impl ConfigService {
    pub fn new(
        store: Arc<StateStore>,
        backend: Arc<dyn Transport>,
        clock: Arc<dyn EntropyClock>,
        product: &str,
    ) -> Result<Self> {
        // Reuse native product validation without installing fallback config.
        Config::from_conf(product, "", &super::super::encode_text("{}")?)?;
        Ok(Self {
            store,
            backend,
            clock,
            product: product.into(),
            gate: Mutex::new(()),
            callbacks: AtomicU64::new(0),
        })
    }
    pub fn cached(&self) -> Result<Option<ConfigSnapshot>> {
        self.store
            .get(OWNER, KIND, &self.product)?
            .map(|v| {
                let r: Record = serde_json::from_value(v)?;
                ConfigSnapshot::parse(&self.product, r.raw, r.expires_ms, r.revision)
            })
            .transpose()
    }
    fn callback(&self) -> Result<String> {
        let mut random = [0; 31];
        self.clock.fill(&mut random)?;
        Ok(format!(
            "__wmjsonp_{}{}",
            &sdk_uuid(random)[2..9],
            self.callbacks.fetch_add(1, Ordering::Relaxed)
        ))
    }
    /// Loader cache is valid at equality. 304 extends the previous top-level ivp;
    /// it never invents missing config or substitutes an unreviewed script build.
    pub async fn load(&self, force: bool) -> Result<ConfigSnapshot> {
        let _gate = self.gate.lock().await;
        let cached = self.cached()?;
        let now = self.clock.now_ms()?;
        if !force
            && let Some(snapshot) = &cached
            && snapshot.expires_ms >= now
        {
            return Ok(snapshot.clone());
        }
        let mut response = None;
        let mut last_error = None;
        for host in [ApiHost::Primary, ApiHost::Secondary] {
            let request = Request {
                host,
                callback: self.callback()?,
                kind: RequestKind::Configuration {
                    product: self.product.clone(),
                    cache_key: cached
                        .as_ref()
                        .map(|c| c.cache_key.clone())
                        .unwrap_or_default(),
                    timestamp_ms: self.clock.now_ms()?,
                },
            };
            match self.backend.send(request).await {
                Ok(value) => {
                    response = Some(value);
                    break;
                }
                Err(e) => last_error = Some(e),
            }
        }
        let response = response
            .ok_or_else(|| last_error.unwrap_or(Error::State("no Watchman configuration host")))?;
        let raw = match response["code"].as_i64() {
            Some(304) => cached
                .as_ref()
                .map(|c| c.raw.clone())
                .ok_or(Error::Response("Watchman 304 without cached configuration"))?,
            Some(200) => response,
            _ => return Err(Error::Response("Watchman configuration rejected")),
        };
        let previous = cached.as_ref().map_or(0, |v| v.revision);
        let revision = previous
            .checked_add(1)
            .ok_or(Error::State("Watchman configuration revision exhausted"))?;
        let mut snapshot = ConfigSnapshot::parse(&self.product, raw.clone(), 0, revision)?;
        snapshot.expires_ms = self
            .clock
            .now_ms()?
            .checked_add(snapshot.interval_ms)
            .ok_or(Error::Response("Watchman config expiry overflow"))?;
        self.store.transaction(|tx| {
            let actual = tx
                .get(OWNER, KIND, &self.product)?
                .map(serde_json::from_value::<Record>)
                .transpose()?
                .map_or(0, |v| v.revision);
            if actual != previous {
                return Err(Error::StaleResponse);
            }
            tx.put(
                OWNER,
                KIND,
                &self.product,
                &serde_json::to_value(Record {
                    revision,
                    expires_ms: snapshot.expires_ms,
                    raw,
                })?,
            )
        })?;
        Ok(snapshot)
    }
}
