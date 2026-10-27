use super::{HttpMethod, Request};
use crate::{Error, Result, protocol::api::ApiFuture};
use serde_json::Value;
use std::time::Duration;

pub trait Transport: Send + Sync {
    /// Return the parsed JSONP value, not HTTP status masquerading as a code.
    fn send(&self, request: Request) -> ApiFuture<'_, Value>;
}
pub struct NativeBackend {
    client: reqwest::Client,
}
impl NativeBackend {
    pub fn new(user_agent: &str) -> Result<Self> {
        if user_agent.len() > 1024
            || user_agent.is_empty()
            || user_agent.contains(['\r', '\n', '\0'])
        {
            return Err(Error::InvalidInput("invalid Watchman user agent"));
        }
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| Error::Transport("Watchman HTTPS initialization failed".into()))?;
        Ok(Self { client })
    }
    async fn execute(&self, request: &Request, url: &str) -> Result<Value> {
        let form = request.form()?;
        let builder = match request.method() {
            HttpMethod::Get => self.client.get(url),
            HttpMethod::Post => self
                .client
                .post(url)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(form),
        };
        // No cookie jar, authentication headers, music credentials, redirect or
        // reqwest automatic retry. URL can only be made by Request in production.
        let mut response = builder
            .header("Accept", "*/*")
            .timeout(Duration::from_millis(request.timeout_ms()))
            .send()
            .await
            .map_err(|_| Error::Transport("Watchman HTTP transfer failed".into()))?;
        if !response.status().is_success() {
            return Err(Error::HttpStatus {
                endpoint: None,
                status: response.status().as_u16(),
                body: None,
            });
        }
        if response
            .content_length()
            .is_some_and(|n| n > super::super::MAX_BYTES as u64)
        {
            return Err(Error::Response("Watchman response exceeds limit"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::Transport("Watchman response interrupted".into()))?
        {
            if chunk.len() > super::super::MAX_BYTES.saturating_sub(bytes.len()) {
                return Err(Error::Response("Watchman response exceeds limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        request.parse_response(&bytes)
    }
}
impl Transport for NativeBackend {
    fn send(&self, request: Request) -> ApiFuture<'_, Value> {
        Box::pin(async move {
            let url = request.url()?;
            self.execute(&request, &url).await
        })
    }
}
#[cfg(test)]
#[path = "http_test.rs"]
mod tests;
