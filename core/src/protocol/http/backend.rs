use super::RequestPlan;
use crate::{Error, Result, protocol::api::ApiFuture};
use std::{collections::BTreeMap, time::Duration};

pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyStatus {
    Complete,
    Interrupted,
    TooLarge,
}

/// Native-only reply; Set-Cookie and refresh headers must never be logged.
pub struct HttpReply {
    pub status: u16,
    pub body: Vec<u8>,
    pub body_status: BodyStatus,
    pub set_cookies: Vec<String>,
    pub refresh_token: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

pub trait HttpBackend: Send + Sync {
    fn post(&self, request: RequestPlan) -> ApiFuture<'_, HttpReply>;
}

pub struct ReqwestBackend {
    client: reqwest::Client,
}
impl ReqwestBackend {
    pub fn new(timeout: Duration) -> Result<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .https_only(true)
            .timeout(timeout)
            .build()
            .map_err(|_| Error::Transport("HTTPS client initialization failed".into()))?;
        Ok(Self { client })
    }
}

impl HttpBackend for ReqwestBackend {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let mut request = self.client.post(plan.url);
            for (name, value) in plan.headers {
                request = request.header(name, value)
            }
            let mut response = request.body(plan.body).send().await.map_err(|error| {
                Error::Transport(
                    if error.is_timeout() {
                        "request timed out"
                    } else if error.is_connect() {
                        "HTTPS connection failed"
                    } else {
                        "HTTP transfer failed"
                    }
                    .into(),
                )
            })?;
            let status = response.status().as_u16();
            if response.status().is_redirection() {
                return Err(Error::Transport("API redirect rejected".into()));
            }
            let oversized = response
                .content_length()
                .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64);
            let set_cookies = response
                .headers()
                .get_all("set-cookie")
                .iter()
                .filter_map(|value| value.to_str().ok().map(str::to_owned))
                .collect();
            let refresh_token = response
                .headers()
                .get("x-refresh-token")
                .map(|value| value.to_str().map(str::to_owned))
                .transpose()
                .map_err(|_| Error::Response("invalid refresh token header"))?;
            let metadata = ["date", "mconfig-info", "mconfig-bucket", "content-type"]
                .into_iter()
                .filter_map(|name| {
                    response
                        .headers()
                        .get(name)
                        .and_then(|value| value.to_str().ok())
                        .map(|value| (name.into(), value.into()))
                })
                .collect();
            let mut body = vec![];
            let mut body_status = if oversized {
                BodyStatus::TooLarge
            } else {
                BodyStatus::Complete
            };
            while body_status == BodyStatus::Complete {
                match response.chunk().await {
                    Ok(None) => break,
                    Ok(Some(chunk)) => {
                        if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
                            body_status = BodyStatus::TooLarge;
                            break;
                        }
                        body.extend_from_slice(&chunk);
                    }
                    Err(_) => {
                        body_status = BodyStatus::Interrupted;
                        break;
                    }
                }
            }
            Ok(HttpReply {
                status,
                body,
                body_status,
                set_cookies,
                refresh_token,
                metadata,
            })
        })
    }
}
