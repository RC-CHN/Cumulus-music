use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use std::collections::BTreeMap;
/// Recovered V1 request signature. The returned signature and canonical input
/// are credentials; callers must not log them or expose them over IPC.
pub fn sign_v1(
    method: &str,
    bucket: &str,
    key: &str,
    headers: &BTreeMap<String, String>,
    params: &BTreeMap<String, String>,
    secret: &str,
) -> Result<String> {
    if !matches!(method, "GET" | "HEAD" | "PUT" | "POST" | "DELETE")
        || params.keys().any(|p| {
            !matches!(
                p.as_str(),
                "uploads" | "uploadId" | "partNumber" | "max-parts" | "part-number-marker"
            )
        })
    {
        return Err(Error::InvalidInput("unsupported OSS signing operation"));
    }
    let mut normalized = BTreeMap::new();
    for (key, value) in headers {
        reqwest::header::HeaderName::from_bytes(key.as_bytes())
            .map_err(|_| Error::InvalidInput("invalid OSS header name"))?;
        reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| Error::InvalidInput("invalid OSS header value"))?;
        if normalized
            .insert(key.to_ascii_lowercase(), value.as_str())
            .is_some()
        {
            return Err(Error::InvalidInput("duplicate case-insensitive OSS header"));
        }
    }
    let get = |name: &str| *normalized.get(name).unwrap_or(&"");
    let date = if get("x-oss-date").is_empty() {
        get("date")
    } else {
        get("x-oss-date")
    };
    let mut canonical = format!(
        "{method}\n{}\n{}\n{date}\n",
        get("content-md5"),
        get("content-type")
    );
    for (key, value) in &normalized {
        if key.starts_with("x-oss-") {
            canonical.push_str(&format!("{key}:{value}\n"));
        }
    }
    canonical.push_str(&format!("/{bucket}/{key}"));
    let signed = params
        .iter()
        .filter(|(k, _)| matches!(k.as_str(), "uploads" | "uploadId" | "partNumber"))
        .map(|(k, v)| {
            if v.is_empty() {
                k.clone()
            } else {
                format!("{k}={v}")
            }
        })
        .collect::<Vec<_>>();
    if !signed.is_empty() {
        canonical.push('?');
        canonical.push_str(&signed.join("&"));
    }
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(secret.as_bytes())
        .map_err(|_| Error::Protocol("invalid OSS signing key"))?;
    mac.update(canonical.as_bytes());
    Ok(STANDARD.encode(mac.finalize().into_bytes()))
}
