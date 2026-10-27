use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
/// Native private persistence only; intentionally no Debug. Never expose the
/// serialized credential tuple to a webview or diagnostic log.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OssAllocation {
    pub(super) bucket: String,
    pub(super) key: String,
    pub(super) endpoint: String,
    pub(super) resource: String,
    pub(super) access: String,
    pub(super) secret: String,
    pub(super) token: String,
}
pub(super) fn text(value: &str, ascii: bool, max: usize) -> Result<()> {
    if value.is_empty()
        || value.len() > max
        || value.chars().any(|c| c < ' ' || c == '\u{7f}')
        || (ascii && !value.is_ascii())
    {
        return Err(Error::InvalidInput("invalid scoped OSS allocation text"));
    }
    Ok(())
}
fn origin(endpoint: &str, bucket: &str) -> Result<String> {
    let bytes = bucket.as_bytes();
    if !(3..=63).contains(&bytes.len())
        || !bytes[0].is_ascii_alphanumeric()
        || !bytes[bytes.len() - 1].is_ascii_alphanumeric()
        || !bytes
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
    {
        return Err(Error::InvalidInput("invalid OSS bucket"));
    }
    text(endpoint, true, 4096)?;
    let url = url::Url::parse(&if endpoint.contains("://") {
        endpoint.to_owned()
    } else {
        format!("https://{endpoint}")
    })
    .map_err(|_| Error::InvalidInput("invalid OSS endpoint"))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|p| p != 80 && p != 443)
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::InvalidInput("unrecognized OSS endpoint"));
    }
    let host = url
        .host_str()
        .ok_or(Error::InvalidInput("missing OSS host"))?;
    let host = host.strip_prefix(&format!("{bucket}.")).unwrap_or(host);
    let region = host
        .strip_prefix("oss-")
        .and_then(|s| s.strip_suffix(".aliyuncs.com"));
    if !region.is_some_and(|s| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    }) {
        return Err(Error::InvalidInput("unrecognized OSS host"));
    }
    Ok(format!("https://{bucket}.{host}"))
}
impl OssAllocation {
    pub fn parse(value: &Value) -> Result<Self> {
        if value["channel"].as_u64() != Some(3) {
            return Err(Error::InvalidInput("allocation is not OSS channel 3"));
        }
        let string = |name: &str| {
            value[name]
                .as_str()
                .map(str::to_owned)
                .ok_or(Error::Response("missing OSS allocation field"))
        };
        let key = value
            .get("objectKey")
            .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
            .or_else(|| value.get("key"))
            .and_then(Value::as_str)
            .ok_or(Error::Response("missing OSS object key"))?
            .to_owned();
        if value
            .get("key")
            .is_some_and(|v| !v.is_null() && v != "" && v != &Value::String(key.clone()))
        {
            return Err(Error::Response("conflicting OSS object keys"));
        }
        let bucket = string("bucket")?;
        let resource = match &value["resourceId"] {
            Value::String(s) => s.clone(),
            Value::Number(n) if n.is_i64() || n.is_u64() => n.to_string(),
            _ => return Err(Error::Response("missing OSS resource ID")),
        };
        let allocation = Self {
            endpoint: origin(&string("endpoint")?, &bucket)?,
            bucket,
            key,
            resource,
            access: string("accessKeyId")?,
            secret: string("secretAccessKey")?,
            token: string("token")?,
        };
        allocation.validate()?;
        Ok(allocation)
    }
    pub(super) fn validate(&self) -> Result<()> {
        origin(&self.endpoint, &self.bucket)?;
        text(&self.key, false, 1023)?;
        // URL parsers normalize dot path segments. Such a key could sign one
        // resource and send another; reject it before creating any request.
        if self.key.starts_with('/') || self.key.split('/').any(|s| matches!(s, "." | "..")) {
            return Err(Error::InvalidInput("unsupported OSS object key path"));
        }
        for value in [&self.access, &self.secret, &self.resource] {
            text(value, true, 4096)?;
        }
        text(&self.token, true, 16384)?;
        Ok(())
    }
    pub fn bucket(&self) -> &str {
        &self.bucket
    }
    pub fn object_key(&self) -> &str {
        &self.key
    }
    pub fn resource_id(&self) -> &str {
        &self.resource
    }
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
    pub fn same_object(&self, other: &Self) -> bool {
        self.bucket == other.bucket
            && self.key == other.key
            && self.endpoint == other.endpoint
            && self.resource == other.resource
    }
    pub(super) fn object_url(&self) -> Result<url::Url> {
        self.validate()?;
        let mut url = url::Url::parse(&origin(&self.endpoint, &self.bucket)?)
            .map_err(|_| Error::State("invalid persisted OSS endpoint"))?;
        url.set_path(&format!("/{}", super::request::encode(&self.key, true)));
        Ok(url)
    }
}
