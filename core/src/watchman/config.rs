use super::{SdkBuild, decode_text};
use crate::{Error, Result, protocol::wire::serialization::js_truthy};
use serde_json::Value;

/// Configuration for this pinned SDK build. No endpoints or script execution
/// are implied by decoding a public configuration snapshot.
#[derive(Clone)]
pub struct Config {
    build: SdkBuild,
    pub(crate) product: String,
    pub(crate) last_used_version: String,
    pub(crate) device_enabled: bool,
    pub(crate) device_ttl_ms: u64,
    pub(crate) config_ttl_ms: u64,
    pub(crate) token_timeout_ms: u64,
    pub(crate) ret: Value,
    blocked_businesses: Vec<String>,
    collection_options: Value,
    decode_elapsed_ms: u64,
}
fn duration(value: Option<&Value>, default: u64) -> Result<u64> {
    match value {
        None | Some(Value::Null) => Ok(default),
        Some(v) if !js_truthy(v) && v.as_u64() != Some(0) => Ok(default),
        Some(v) => v
            .as_u64()
            .filter(|n| *n <= 9_007_199_254_740_991)
            .ok_or(Error::Response(
                "unsupported Watchman configuration duration",
            )),
    }
}
impl Config {
    pub fn from_conf(product: &str, last_used_version: &str, encoded: &str) -> Result<Self> {
        Self::from_conf_for(SdkBuild::Af2952a4, product, last_used_version, encoded)
    }
    pub fn from_conf_for(
        build: SdkBuild,
        product: &str,
        last_used_version: &str,
        encoded: &str,
    ) -> Result<Self> {
        if product.is_empty()
            || product.len() > 64
            || !product.bytes().all(|b| b.is_ascii_alphanumeric())
            || last_used_version.len() > 100
            || last_used_version.contains(['\r', '\n', '\0'])
        {
            return Err(Error::InvalidInput("invalid Watchman product or version"));
        }
        let started = std::time::Instant::now();
        let decoded = decode_text(encoded)?;
        let decode_elapsed_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        let value: Value = serde_json::from_str(&decoded)?;
        if !value.is_object() {
            return Err(Error::Response("Watchman conf is not an object"));
        }
        if let Some(pn) = value.get("pn")
            && pn.as_str() != Some(product)
        {
            return Err(Error::Response("Watchman product mismatch"));
        }
        let ret = value
            .get("ret")
            .filter(|v| js_truthy(v) || v.as_u64() == Some(0))
            .cloned()
            .unwrap_or(Value::from(1));
        if ret.is_object() || ret.is_array() {
            return Err(Error::Response("unsupported Watchman ret"));
        }
        Ok(Self {
            build,
            product: product.into(),
            last_used_version: last_used_version.into(),
            device_enabled: value["edc"].as_bool().unwrap_or(true),
            device_ttl_ms: duration(value.get("dtvp"), 3_600_000)?,
            config_ttl_ms: duration(value.get("ivp"), 86_400_000)?,
            token_timeout_ms: duration(value.get("tto"), 2000)?,
            ret,
            blocked_businesses: value["bl"]
                .as_str()
                .map(|v| v.split(',').map(str::to_owned).collect())
                .unwrap_or_default(),
            collection_options: value,
            decode_elapsed_ms,
        })
    }
    pub fn product(&self) -> &str {
        &self.product
    }
    pub fn build_version(&self) -> &'static str {
        self.build.version()
    }
    pub fn build(&self) -> SdkBuild {
        self.build
    }
    pub fn device_enabled(&self) -> bool {
        self.device_enabled
    }
    pub fn device_ttl_ms(&self) -> u64 {
        self.device_ttl_ms
    }
    pub fn config_ttl_ms(&self) -> u64 {
        self.config_ttl_ms
    }
    pub fn token_timeout_ms(&self) -> u64 {
        self.token_timeout_ms
    }
    pub fn ret(&self) -> &Value {
        &self.ret
    }
    pub fn business_suppressed(&self, business: &str) -> bool {
        self.blocked_businesses.iter().any(|b| b == business)
    }
    /// Reviewed raw collection switches and sampling limits for a native host.
    /// This does not evaluate any collector or configure a remote URL.
    pub fn collection_options(&self) -> &Value {
        &self.collection_options
    }
    pub(crate) fn decode_elapsed_ms(&self) -> u64 {
        self.decode_elapsed_ms
    }
}
