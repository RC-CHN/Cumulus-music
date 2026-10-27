//! Official M/402 path matching and fresh per-request token injection.
//! A token provider is an explicit dependency, not a fabricated SDK token.
use super::mconfig::PcConfig;
use crate::{Error, Result, protocol::api::ApiFuture};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

pub(crate) fn validate_configuration(config: &Value) -> Result<()> {
    let Some(groups) = config.get("yidun#paths").and_then(|v| v.get("whiteList")) else {
        return Ok(());
    };
    for group in groups
        .as_array()
        .ok_or(Error::Response("invalid protected path configuration"))?
    {
        let business = group["businessId"]
            .as_str()
            .ok_or(Error::Response("invalid business ID"))?;
        if business.len() > 128 || business.contains(['\r', '\n', '\0']) {
            return Err(Error::Response("invalid business ID"));
        }
        for item in group["list"]
            .as_array()
            .ok_or(Error::Response("invalid protected path group"))?
        {
            if !item
                .as_str()
                .is_some_and(|s| s.len() <= 4096 && !s.contains(['\r', '\n', '\0']))
            {
                return Err(Error::Response("invalid protected path entry"));
            }
        }
    }
    Ok(())
}

pub fn business_for_path(path: &str, config: &Value, api_host: &str) -> Result<Option<String>> {
    let path = path
        .strip_prefix(api_host)
        .filter(|path| path.starts_with('/'))
        .unwrap_or(path);
    // M/402 compares substr(0, 6) with the FIVE-character string "batch".
    // Therefore only the exact short value passes, not "batch/...". The old
    // Python startswith implementation broadened this branch accidentally.
    if !path.starts_with("/api/") && path != "batch" {
        return Ok(None);
    }
    if path.contains(['?', '#']) {
        return Err(Error::InvalidInput("expected a normalized API path"));
    }
    let path = path.strip_suffix('/').unwrap_or(path);
    let Some(groups) = config
        .get("yidun#paths")
        .and_then(|value| value.get("whiteList"))
    else {
        return Ok(None);
    };
    let groups = groups
        .as_array()
        .ok_or(Error::Response("invalid protected path configuration"))?;
    let mut matched = None;
    for group in groups {
        let entries = group["list"]
            .as_array()
            .ok_or(Error::Response("invalid protected path group"))?;
        let mut matches = false;
        for entry in entries {
            let entry = entry
                .as_str()
                .ok_or(Error::Response("invalid protected path entry"))?;
            matches |= if entry.ends_with('/') {
                path.starts_with(entry)
            } else {
                path == entry
            };
        }
        if matches {
            let business = group["businessId"]
                .as_str()
                .ok_or(Error::Response("invalid business ID"))?;
            matched = (!business.is_empty()).then(|| business.to_owned());
        }
    }
    // Official forEach permits the last matching group to replace earlier ones.
    Ok(matched)
}

pub trait TokenProvider: Send + Sync {
    fn token<'a>(&'a self, business: &'a str) -> ApiFuture<'a, String>;
}

pub struct PreparedRequest {
    pub payload: Value,
    pub headers: BTreeMap<String, String>,
}

pub struct RequestContext {
    config: Configuration,
    api_host: String,
    token_provider: Arc<dyn TokenProvider>,
}
enum Configuration {
    Fixed(Value),
    Pc {
        service: Arc<PcConfig>,
        app_version: String,
    },
}

impl RequestContext {
    pub fn api_host(&self) -> &str {
        &self.api_host
    }
    /// The caller must separately verify configuration origin and platform.
    pub fn new(config: Value, api_host: String, token_provider: Arc<dyn TokenProvider>) -> Self {
        Self {
            config: Configuration::Fixed(config),
            api_host,
            token_provider,
        }
    }

    /// Native-only verified PC configuration center shared by the active
    /// transport and isolated login candidates. No frontend policy injection.
    pub fn from_pc(
        service: Arc<PcConfig>,
        app_version: &str,
        token_provider: Arc<dyn TokenProvider>,
    ) -> Result<Self> {
        service.snapshot()?.request_header(app_version)?;
        Ok(Self {
            config: Configuration::Pc {
                service,
                app_version: app_version.into(),
            },
            api_host: super::http::ApiOrigin::Pc.as_str().into(),
            token_provider,
        })
    }
    pub fn metadata_headers(&self) -> Result<BTreeMap<String, String>> {
        match &self.config {
            Configuration::Fixed(_) => Ok(BTreeMap::new()),
            Configuration::Pc {
                service,
                app_version,
            } => Ok(BTreeMap::from([(
                "MConfig-Info".into(),
                service.snapshot()?.request_header(app_version)?,
            )])),
        }
    }
    pub(crate) fn observe_response(&self, metadata: BTreeMap<String, String>) {
        if let Configuration::Pc { service, .. } = &self.config {
            service.observe(metadata);
        }
    }

    pub async fn prepare(&self, path: &str, payload: &Value) -> Result<PreparedRequest> {
        self.prepare_inner(path, payload, false).await
    }
    /// A formally enabled PC write cannot silently treat an unavailable
    /// configuration as an empty protected-path policy.
    pub(crate) async fn prepare_configured(
        &self,
        path: &str,
        payload: &Value,
    ) -> Result<PreparedRequest> {
        self.prepare_inner(path, payload, true).await
    }
    async fn prepare_inner(
        &self,
        path: &str,
        payload: &Value,
        require_config: bool,
    ) -> Result<PreparedRequest> {
        if !payload.is_object() {
            return Err(Error::InvalidInput("request payload must be an object"));
        }
        let mut payload = payload.clone();
        let snapshot = match &self.config {
            Configuration::Pc { service, .. } => Some(service.snapshot()?),
            _ => None,
        };
        let empty = serde_json::json!({});
        let (config, mut headers) = match &self.config {
            Configuration::Fixed(config) => (config, BTreeMap::new()),
            Configuration::Pc { app_version, .. } => {
                let snapshot = snapshot
                    .as_ref()
                    .ok_or(Error::State("PC configuration snapshot unavailable"))?;
                if require_config && snapshot.data.is_none() {
                    return Err(Error::State("PC request configuration is not ready"));
                }
                (
                    snapshot.data.as_ref().unwrap_or(&empty),
                    BTreeMap::from([(
                        "MConfig-Info".into(),
                        snapshot.request_header(app_version)?,
                    )]),
                )
            }
        };
        if let Some(business) = business_for_path(path, config, &self.api_host)? {
            let token = self.token_provider.token(&business).await?;
            if token.is_empty() || token.contains(['\r', '\n', '\0']) {
                return Err(Error::State(
                    "token provider supplied no valid token; request was not sent",
                ));
            }
            payload["checkToken"] = token.clone().into();
            headers.insert("X-antiCheatToken".into(), token);
        }
        if let Configuration::Pc { service, .. } = &self.config
            && snapshot.as_ref().is_some_and(|old| {
                service
                    .snapshot()
                    .map_or(true, |new| new.generation != old.generation)
            })
        {
            return Err(Error::StaleResponse);
        }
        Ok(PreparedRequest { payload, headers })
    }
}
