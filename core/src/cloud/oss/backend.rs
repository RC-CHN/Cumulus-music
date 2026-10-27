use super::*;
use crate::protocol::api::ApiFuture;
use sha2::Digest;
use std::{collections::BTreeMap, time::Duration};
#[cfg(test)]
#[path = "http_test.rs"]
mod tests;
pub trait OssBackend: Send + Sync {
    fn request(&self, request: OssRequest) -> ApiFuture<'_, OssReply>;
}
pub struct ReqwestOssBackend {
    client: reqwest::Client,
}
impl ReqwestOssBackend {
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
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|_| Error::Transport("OSS client initialization failed".into()))?;
        Ok(Self { client })
    }
}
impl OssBackend for ReqwestOssBackend {
    fn request(&self, request: OssRequest) -> ApiFuture<'_, OssReply> {
        Box::pin(async move {
            let method = reqwest::Method::from_bytes(request.method.as_bytes())
                .map_err(|_| Error::InvalidInput("invalid OSS method"))?;
            let mut call = self.client.request(method, &request.url);
            for (key, value) in &request.headers {
                let name = reqwest::header::HeaderName::from_bytes(key.as_bytes())
                    .map_err(|_| Error::InvalidInput("invalid OSS header name"))?;
                let mut header = reqwest::header::HeaderValue::from_str(value)
                    .map_err(|_| Error::InvalidInput("invalid OSS header value"))?;
                if key.eq_ignore_ascii_case("authorization")
                    || key.eq_ignore_ascii_case("x-oss-security-token")
                {
                    header.set_sensitive(true);
                }
                call = call.header(name, header);
            }
            if let Some(body) = request.body {
                call = call.body(body);
            }
            let mut response = call
                .send()
                .await
                .map_err(|_| Error::Transport("OSS request failed".into()))?;
            let status = response.status().as_u16();
            let mut headers = BTreeMap::new();
            for name in response.headers().keys() {
                if response.headers().get_all(name).iter().count() != 1
                    && matches!(
                        name.as_str(),
                        "etag"
                            | "content-length"
                            | "x-oss-meta-cumulus-upload"
                            | "x-oss-hash-crc64ecma"
                    )
                {
                    return Err(Error::Response(
                        "duplicate OSS identity or integrity header",
                    ));
                }
                if let Ok(value) = response.headers()[name].to_str() {
                    headers.insert(name.as_str().into(), value.into());
                }
            }
            let object = request.object_bytes.filter(|_| status == request.expected);
            let mut bytes = 0u64;
            let mut body = Vec::new();
            let mut md5 = md5::Md5::new();
            let mut sha = sha2::Sha256::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| Error::Transport("OSS response interrupted".into()))?
            {
                bytes += chunk.len() as u64;
                if bytes > object.unwrap_or(1024 * 1024) {
                    return Err(Error::Response("OSS response exceeds size limit"));
                }
                if object.is_some() {
                    md5.update(&chunk);
                    sha.update(&chunk);
                } else {
                    body.extend_from_slice(&chunk);
                }
            }
            let digest = object.map(|_| ObjectDigest {
                bytes,
                md5: hex::encode(md5.finalize()),
                sha256: hex::encode(sha.finalize()),
            });
            let result = OssReply {
                status,
                headers,
                body,
                digest,
            };
            result.check(request.expected)?;
            Ok(result)
        })
    }
}
