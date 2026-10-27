use super::{ConfigUrl, model::MAX_BYTES};
use crate::{Error, Result, protocol::api::ApiFuture};
use serde_json::Value;
use std::time::Duration;
pub trait ConfigBackend: Send + Sync {
    fn get<'a>(&'a self, url: &'a ConfigUrl) -> ApiFuture<'a, Value>;
}
pub struct NativeConfigBackend(reqwest::Client);
impl NativeConfigBackend {
    pub fn new(user_agent: &str) -> Result<Self> {
        if user_agent.is_empty()
            || user_agent.len() > 1024
            || user_agent.contains(['\r', '\n', '\0'])
        {
            return Err(Error::InvalidInput("invalid MConfig user agent"));
        }
        Ok(Self(
            reqwest::Client::builder()
                .https_only(true)
                .user_agent(user_agent)
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|_| Error::Transport("MConfig HTTP initialization failed".into()))?,
        ))
    }
}
impl ConfigBackend for NativeConfigBackend {
    fn get<'a>(&'a self, url: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let mut response = self
                .0
                .get(url.as_str())
                .send()
                .await
                .map_err(|_| Error::Transport("MConfig fetch failed".into()))?;
            if !response.status().is_success() {
                return Err(Error::HttpStatus {
                    endpoint: None,
                    status: response.status().as_u16(),
                    body: None,
                });
            }
            if response
                .content_length()
                .is_some_and(|n| n > MAX_BYTES as u64)
            {
                return Err(Error::Response("MConfig exceeds limit"));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| Error::Transport("MConfig interrupted".into()))?
            {
                if chunk.len() > MAX_BYTES.saturating_sub(bytes.len()) {
                    return Err(Error::Response("MConfig exceeds limit"));
                }
                bytes.extend_from_slice(&chunk);
            }
            serde_json::from_slice(&bytes).map_err(|_| Error::Response("MConfig is not JSON"))
        })
    }
}
