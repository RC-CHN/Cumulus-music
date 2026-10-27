use super::*;
use crate::protocol::api::ApiFuture;
use serde_json::Value;
use std::time::Duration;
#[cfg(test)]
#[path = "http_test.rs"]
mod tests;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NosMethod {
    Query,
    Chunk,
}
/// Native transport request; has no Debug/Serialize because it contains a token.
#[derive(Clone)]
pub struct NosRequest {
    url: String,
    token: String,
    md5: String,
    content_type: String,
    method: NosMethod,
    body: Vec<u8>,
}
impl NosRequest {
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn scoped_token(&self) -> &str {
        &self.token
    }
    pub fn md5(&self) -> &str {
        &self.md5
    }
    pub fn content_type(&self) -> &str {
        &self.content_type
    }
    pub fn method(&self) -> NosMethod {
        self.method
    }
    pub fn body(&self) -> &[u8] {
        &self.body
    }
    pub(super) fn new(job: &Job, data: Option<Vec<u8>>) -> Result<Self> {
        job.validate()?;
        let node = NosNode::parse(job.node.as_str())?;
        let mut url =
            url::Url::parse(node.as_str()).map_err(|_| Error::State("invalid NOS node"))?;
        url.path_segments_mut()
            .map_err(|_| Error::State("invalid NOS path"))?
            .push(&job.allocation.bucket)
            .push(&job.allocation.key);
        let query = data.is_none();
        let mut params = url::form_urlencoded::Serializer::new(String::new());
        if query {
            if job.context.is_empty() {
                return Err(Error::State("query requires NOS context"));
            }
            params.append_pair("context", &job.context);
        } else {
            params
                .append_pair("offset", &job.offset.to_string())
                .append_pair(
                    "complete",
                    if job.pending.as_ref().is_some_and(|p| p.complete) {
                        "true"
                    } else {
                        "false"
                    },
                );
            if !job.context.is_empty() {
                params.append_pair("context", &job.context);
            }
        }
        params.append_pair("version", "1.0");
        let params = params.finish();
        url.set_query(Some(&if query {
            format!("uploadContext&{params}")
        } else {
            params
        }));
        Ok(Self {
            url: url.into(),
            token: job.allocation.token.clone(),
            md5: job.source.md5.clone(),
            content_type: job.content_type.clone(),
            method: if query {
                NosMethod::Query
            } else {
                NosMethod::Chunk
            },
            body: data.unwrap_or_default(),
        })
    }
}
pub trait NosBackend: Send + Sync {
    fn discover<'a>(&'a self, bucket: &'a str) -> ApiFuture<'a, NosNode>;
    fn request(&self, request: NosRequest) -> ApiFuture<'_, Value>;
}
pub struct ReqwestNosBackend {
    client: reqwest::Client,
}
impl ReqwestNosBackend {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| Error::Transport("NOS client initialization failed".into()))?;
        Ok(Self { client })
    }
    async fn json(&self, request: reqwest::RequestBuilder) -> Result<Value> {
        let mut response = request
            .send()
            .await
            .map_err(|_| Error::Transport("NOS request failed".into()))?;
        if response.status().as_u16() != 200 {
            return Err(Error::HttpStatus {
                endpoint: None,
                status: response.status().as_u16(),
                body: None,
            });
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::Transport("NOS response interrupted".into()))?
        {
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return Err(Error::Response("NOS response too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes)?;
        if !value.is_object() {
            return Err(Error::Response("NOS response is not an object"));
        }
        Ok(value)
    }
}
impl NosBackend for ReqwestNosBackend {
    fn discover<'a>(&'a self, bucket: &'a str) -> ApiFuture<'a, NosNode> {
        Box::pin(async move {
            types::valid_bucket(bucket)?;
            // Only public bucket routing; neither account Cookie nor object token.
            for host in [
                "wanproxy.127.net",
                "wanproxy-hz.127.net",
                "wanproxy-bj.127.net",
                "wanproxy-oversea.127.net",
            ] {
                for scheme in ["https", "http"] {
                    let mut url = url::Url::parse(&format!("{scheme}://{host}/lbs"))
                        .map_err(|_| Error::State("invalid LBS URL"))?;
                    url.query_pairs_mut()
                        .append_pair("version", "1.0")
                        .append_pair("bucketname", bucket);
                    let call = self.client.get(url);
                    if let Ok(value) = self.json(call).await
                        && let Some(nodes) = value["upload"].as_array()
                    {
                        for node in nodes {
                            if let Some(node) = node.as_str()
                                && let Ok(node) = NosNode::parse(node)
                            {
                                return Ok(node);
                            }
                        }
                    }
                }
            }
            Err(Error::Response("no supported NOS upload node"))
        })
    }
    fn request(&self, request: NosRequest) -> ApiFuture<'_, Value> {
        Box::pin(async move {
            let mut token = reqwest::header::HeaderValue::from_str(&request.token)
                .map_err(|_| Error::InvalidInput("invalid scoped NOS token"))?;
            token.set_sensitive(true);
            let call = if request.method == NosMethod::Query {
                self.client.get(&request.url)
            } else {
                self.client
                    .post(&request.url)
                    .header("Content-Type", &request.content_type)
                    .header("Content-MD5", &request.md5)
                    .body(request.body)
            };
            self.json(
                call.header("x-nos-token", token)
                    .header("Accept-Encoding", "identity"),
            )
            .await
        })
    }
}
