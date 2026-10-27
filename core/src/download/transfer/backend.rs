use super::MediaUrl;
use crate::{Error, Result, protocol::api::ApiFuture};
use std::{collections::BTreeMap, time::Duration};

#[derive(Clone)]
pub struct MediaRequest {
    pub url: MediaUrl,
    pub offset: u64,
    pub if_range: Option<String>,
}
pub trait MediaBody: Send {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize>;
}
/// Only response facts used by range validation are retained. No Cookie jar,
/// API authorization header, refresh receipt or Set-Cookie is exposed here.
pub struct MediaReply {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Box<dyn MediaBody>,
}
pub trait MediaBackend: Send + Sync {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply>;
}

pub struct ReqwestMediaBackend {
    client: reqwest::Client,
}
impl ReqwestMediaBackend {
    pub fn new(timeout: Duration) -> Result<Self> {
        if timeout.is_zero() || timeout > Duration::from_secs(300) {
            return Err(Error::InvalidInput(
                "media timeout must be within 0..300 seconds",
            ));
        }
        let client = client_builder(timeout)
            .https_only(true)
            .build()
            .map_err(|_| Error::Transport("media client initialization failed".into()))?;
        Ok(Self { client })
    }
}
fn client_builder(timeout: Duration) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .referer(false)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .connect_timeout(timeout)
        .read_timeout(timeout)
}

#[cfg(test)]
#[path = "backend_test.rs"]
mod tests;
impl MediaBackend for ReqwestMediaBackend {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            let mut call = self
                .client
                .get(request.url.as_str())
                .header("User-Agent", "Cumulus/0.1")
                .header("Accept-Encoding", "identity");
            if request.offset != 0 {
                call = call.header("Range", format!("bytes={}-", request.offset));
                if let Some(validator) = request.if_range {
                    call = call.header("If-Range", validator);
                }
            }
            let response = call
                .send()
                .await
                .map_err(|_| Error::Transport("media request failed".into()))?;
            let status = response.status().as_u16();
            let mut headers = BTreeMap::new();
            for key in [
                "content-length",
                "content-range",
                "content-encoding",
                "etag",
                "last-modified",
            ] {
                let mut all = response.headers().get_all(key).iter();
                if let Some(value) = all.next() {
                    if all.next().is_some() {
                        return Err(Error::Response("ambiguous media header"));
                    }
                    headers.insert(
                        key.into(),
                        value
                            .to_str()
                            .map_err(|_| Error::Response("non-text media header"))?
                            .into(),
                    );
                }
            }
            Ok(MediaReply {
                status,
                headers,
                body: Box::new(Body {
                    response,
                    pending: Vec::new(),
                    offset: 0,
                }),
            })
        })
    }
}
struct Body {
    response: reqwest::Response,
    pending: Vec<u8>,
    offset: usize,
}
impl MediaBody for Body {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            if buffer.is_empty() {
                return Ok(0);
            }
            while self.offset == self.pending.len() {
                let Some(chunk) = self
                    .response
                    .chunk()
                    .await
                    .map_err(|_| Error::Transport("media body interrupted".into()))?
                else {
                    return Ok(0);
                };
                self.pending = chunk.to_vec();
                self.offset = 0;
            }
            let count = buffer.len().min(self.pending.len() - self.offset);
            buffer[..count].copy_from_slice(&self.pending[self.offset..self.offset + count]);
            self.offset += count;
            Ok(count)
        })
    }
}
