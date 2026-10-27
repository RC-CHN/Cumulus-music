use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use md5::Digest;
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartTag {
    pub number: u32,
    pub etag: String,
}
pub enum OssOperation {
    Initiate,
    Put(Vec<u8>),
    Part {
        upload_id: String,
        number: u32,
        data: Vec<u8>,
    },
    Parts {
        upload_id: String,
        marker: u32,
    },
    Complete {
        upload_id: String,
        parts: Vec<PartTag>,
    },
    Head,
    Verify {
        bytes: u64,
    },
    Abort {
        upload_id: String,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OssRequestKind {
    Initiate,
    Put,
    Part,
    Parts,
    Complete,
    Head,
    Verify,
    Abort,
}
/// Native-only signed request. No Debug or Serialize: headers contain STS
/// credentials and a signature, and its URL identifies the scoped object.
#[derive(Clone)]
pub struct OssRequest {
    pub(super) method: &'static str,
    pub(super) url: String,
    pub(super) headers: BTreeMap<String, String>,
    pub(super) body: Option<Vec<u8>>,
    pub(super) expected: u16,
    pub(super) object_bytes: Option<u64>,
    kind: OssRequestKind,
}
pub(super) fn encode(input: &str, slash: bool) -> String {
    let mut encoded = String::new();
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) || (slash && byte == b'/') {
            encoded.push(byte as char)
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 15) as usize] as char);
        }
    }
    encoded
}
pub(super) fn valid_upload_id(id: &str) -> Result<()> {
    allocation::text(id, true, 2048)
}
pub(super) fn etag(input: &str) -> Result<String> {
    let value = input
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(input);
    if value.len() != 32 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::Response("invalid OSS part ETag"));
    }
    Ok(value.to_ascii_lowercase())
}
impl OssRequest {
    pub fn new(a: &OssAllocation, id: &str, operation: OssOperation, date: &str) -> Result<Self> {
        a.validate()?;
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return Err(Error::InvalidInput("invalid OSS task marker"));
        }
        allocation::text(date, true, 128)?;
        let mut headers = BTreeMap::from([
            ("Date".into(), date.into()),
            ("x-oss-security-token".into(), a.token.clone()),
            ("Accept-Encoding".into(), "identity".into()),
        ]);
        let mut params = BTreeMap::<String, String>::new();
        let mut body = None;
        let mut expected = 200;
        let mut object_bytes = None;
        let (method, kind) = match operation {
            OssOperation::Initiate => {
                params.insert("uploads".into(), String::new());
                body = Some(Vec::new());
                ("POST", OssRequestKind::Initiate)
            }
            OssOperation::Put(data) => {
                body = Some(data);
                ("PUT", OssRequestKind::Put)
            }
            OssOperation::Part {
                upload_id,
                number,
                data,
            } => {
                valid_upload_id(&upload_id)?;
                if !(1..=10000).contains(&number) || data.is_empty() {
                    return Err(Error::InvalidInput("invalid OSS part"));
                }
                params.insert("uploadId".into(), upload_id);
                params.insert("partNumber".into(), number.to_string());
                body = Some(data);
                ("PUT", OssRequestKind::Part)
            }
            OssOperation::Parts { upload_id, marker } => {
                valid_upload_id(&upload_id)?;
                if marker >= 10000 {
                    return Err(Error::InvalidInput("invalid OSS part marker"));
                }
                params.insert("uploadId".into(), upload_id);
                params.insert("max-parts".into(), "1000".into());
                if marker > 0 {
                    params.insert("part-number-marker".into(), marker.to_string());
                }
                ("GET", OssRequestKind::Parts)
            }
            OssOperation::Complete { upload_id, parts } => {
                valid_upload_id(&upload_id)?;
                if parts.is_empty() || parts.len() > 10000 {
                    return Err(Error::InvalidInput("invalid OSS completion parts"));
                }
                let mut xml = String::from(
                    "<?xml version='1.0' encoding='utf-8'?>\n<CompleteMultipartUpload>",
                );
                for (index, part) in parts.iter().enumerate() {
                    if part.number != index as u32 + 1 {
                        return Err(Error::InvalidInput(
                            "OSS completion parts are not contiguous",
                        ));
                    }
                    xml.push_str(&format!(
                        "<Part><PartNumber>{}</PartNumber><ETag>\"{}\"</ETag></Part>",
                        part.number,
                        etag(&part.etag)?.to_ascii_uppercase()
                    ));
                }
                xml.push_str("</CompleteMultipartUpload>");
                body = Some(xml.into_bytes());
                headers.insert("Content-Type".into(), "application/xml".into());
                params.insert("uploadId".into(), upload_id);
                ("POST", OssRequestKind::Complete)
            }
            OssOperation::Head => ("HEAD", OssRequestKind::Head),
            OssOperation::Verify { bytes } => {
                if bytes == 0 || bytes > 512 * 1024 * 1024 {
                    return Err(Error::InvalidInput("invalid OSS verification size"));
                }
                object_bytes = Some(bytes);
                ("GET", OssRequestKind::Verify)
            }
            OssOperation::Abort { upload_id } => {
                valid_upload_id(&upload_id)?;
                params.insert("uploadId".into(), upload_id);
                expected = 204;
                ("DELETE", OssRequestKind::Abort)
            }
        };
        if matches!(kind, OssRequestKind::Initiate | OssRequestKind::Put) {
            headers.insert("x-oss-meta-cumulus-upload".into(), id.into());
        }
        if matches!(
            kind,
            OssRequestKind::Initiate | OssRequestKind::Put | OssRequestKind::Complete
        ) {
            headers.insert("x-oss-forbid-overwrite".into(), "true".into());
        }
        if let Some(body) = &body {
            if body.len() > 64 * 1024 * 1024 || (kind == OssRequestKind::Put && body.is_empty()) {
                return Err(Error::InvalidInput("invalid OSS body length"));
            }
            headers
                .entry("Content-Type".into())
                .or_insert_with(|| "application/octet-stream".into());
            headers.insert(
                "Content-MD5".into(),
                STANDARD.encode(md5::Md5::digest(body)),
            );
            headers.insert("Content-Length".into(), body.len().to_string());
        }
        let signature = sign_v1(method, &a.bucket, &a.key, &headers, &params, &a.secret)?;
        headers.insert(
            "Authorization".into(),
            format!("OSS {}:{signature}", a.access),
        );
        let mut url = a.object_url()?;
        if !params.is_empty() {
            url.set_query(Some(
                &params
                    .iter()
                    .map(|(k, v)| {
                        if v.is_empty() {
                            encode(k, false)
                        } else {
                            format!("{}={}", encode(k, false), encode(v, false))
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("&"),
            ));
        }
        Ok(Self {
            method,
            url: url.into(),
            headers,
            body,
            expected,
            object_bytes,
            kind,
        })
    }
    pub fn method(&self) -> &str {
        self.method
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn headers(&self) -> &BTreeMap<String, String> {
        &self.headers
    }
    pub fn body(&self) -> Option<&[u8]> {
        self.body.as_deref()
    }
    pub fn kind(&self) -> OssRequestKind {
        self.kind
    }
    pub fn object_bytes(&self) -> Option<u64> {
        self.object_bytes
    }
}
