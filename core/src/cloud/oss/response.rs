use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectDigest {
    pub bytes: u64,
    pub md5: String,
    pub sha256: String,
}
/// Native response; body/headers may identify the scoped object. Not an IPC DTO.
pub struct OssReply {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub digest: Option<ObjectDigest>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartInfo {
    pub number: u32,
    pub bytes: u64,
    pub etag: String,
}
pub struct PartsPage {
    pub parts: Vec<PartInfo>,
    pub next_marker: Option<u32>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Completion {
    pub etag: Option<String>,
    pub crc64_verified: bool,
    pub content_verified: bool,
}
impl OssReply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
    pub fn check(&self, expected: u16) -> Result<()> {
        if self.status != expected {
            return Err(http_error(self.status, &self.body));
        }
        if self.body.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'<')
            && xml::parse(&self.body)?.name == "Error"
        {
            return Err(http_error(self.status, &self.body));
        }
        Ok(())
    }
    pub fn initiated(&self, a: &OssAllocation) -> Result<String> {
        self.check(200)?;
        let node = xml::parse(&self.body)?;
        if node.name != "InitiateMultipartUploadResult" {
            return Err(Error::Response("unexpected OSS initialization response"));
        }
        node.scope(a)?;
        let id = node.field("UploadId")?;
        request::valid_upload_id(id)?;
        Ok(id.into())
    }
    pub fn part_etag(&self) -> Result<String> {
        self.check(200)?;
        request::etag(
            self.header("etag")
                .ok_or(Error::Response("missing OSS ETag"))?,
        )
    }
    pub fn put_completed(&self, md5: &str, crc: u64) -> Result<Completion> {
        if self.part_etag()? != md5 {
            return Err(Error::Media("OSS object MD5 mismatch"));
        }
        Ok(Completion {
            etag: self.header("etag").map(str::to_owned),
            crc64_verified: check_crc64(self.header("x-oss-hash-crc64ecma"), crc)?,
            content_verified: false,
        })
    }
    pub fn parts(&self, a: &OssAllocation, upload_id: &str, marker: u32) -> Result<PartsPage> {
        self.check(200)?;
        let node = xml::parse(&self.body)?;
        if node.name != "ListPartsResult" {
            return Err(Error::Response("unexpected OSS part list"));
        }
        node.scope(a)?;
        if node.field("UploadId")? != upload_id {
            return Err(Error::Response(
                "OSS response refers to another multipart upload",
            ));
        }
        let mut parts = Vec::new();
        let mut last = marker;
        for part in node.children.iter().filter(|n| n.name == "Part") {
            let number = decimal(part.field("PartNumber")?)?
                .try_into()
                .map_err(|_| Error::Response("OSS part number overflow"))?;
            let bytes = decimal(part.field("Size")?)?;
            if number <= last || number > 10000 || bytes == 0 || parts.len() >= 1000 {
                return Err(Error::Response("invalid OSS part order or size"));
            }
            parts.push(PartInfo {
                number,
                bytes,
                etag: request::etag(part.field("ETag")?)?,
            });
            last = number;
        }
        let next_marker = match node.field("IsTruncated")? {
            "false" => None,
            "true" if !parts.is_empty() => {
                if decimal(node.field("NextPartNumberMarker")?)? != u64::from(last) || last >= 10000
                {
                    return Err(Error::Response("OSS pagination marker skips parts"));
                }
                Some(last)
            }
            _ => return Err(Error::Response("invalid OSS pagination")),
        };
        Ok(PartsPage { parts, next_marker })
    }
    pub fn multipart_completed(&self, a: &OssAllocation, crc: u64) -> Result<Completion> {
        self.check(200)?;
        let node = xml::parse(&self.body)?;
        if node.name != "CompleteMultipartUploadResult" {
            return Err(Error::Response("unexpected OSS completion response"));
        }
        node.scope(a)?;
        let tag = node.field("ETag")?;
        allocation::text(tag, true, 4096)?;
        Ok(Completion {
            etag: Some(tag.into()),
            crc64_verified: check_crc64(self.header("x-oss-hash-crc64ecma"), crc)?,
            content_verified: false,
        })
    }
    pub fn object_headers(&self, id: &str, size: u64) -> Result<()> {
        self.check(200)?;
        if self.header("x-oss-meta-cumulus-upload") != Some(id)
            || self.header("content-length").map(decimal).transpose()? != Some(size)
        {
            return Err(Error::MutationUncertain);
        }
        Ok(())
    }
    pub fn verified_object(
        &self,
        id: &str,
        size: u64,
        md5: &str,
        sha256: &str,
    ) -> Result<Completion> {
        self.object_headers(id, size)?;
        let digest = self
            .digest
            .as_ref()
            .ok_or(Error::Response("missing OSS object digest"))?;
        if digest.bytes != size || digest.md5 != md5 || digest.sha256 != sha256 {
            return Err(Error::MutationUncertain);
        }
        Ok(Completion {
            etag: None,
            crc64_verified: false,
            content_verified: true,
        })
    }
}
fn decimal(value: &str) -> Result<u64> {
    if value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit()) {
        return Err(Error::Response("invalid OSS decimal field"));
    }
    value
        .parse()
        .map_err(|_| Error::Response("OSS decimal field overflow"))
}
pub(super) fn http_error(status: u16, raw: &[u8]) -> Error {
    let code = xml::parse(raw)
        .ok()
        .and_then(|node| node.field("Code").ok().map(str::to_owned))
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 80
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        });
    Error::HttpStatus {
        endpoint: None,
        status,
        body: code.map(|code| Box::new(serde_json::json!({"code":code}))),
    }
}
