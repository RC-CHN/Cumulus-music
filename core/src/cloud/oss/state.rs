use super::*;
use crate::cloud::source::Source;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OssStatus {
    Queued,
    Initiating,
    Uploading,
    Paused,
    Completing,
    Ambiguous,
    Error,
    Uploaded,
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OssControl {
    Pause,
    Cancel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OssPending {
    Initiate,
    Put,
    Parts,
    Complete,
    Abort,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Pending {
    Initiate,
    Put,
    Parts { numbers: Vec<u32> },
    Complete,
    Abort,
}
impl Pending {
    fn kind(&self) -> OssPending {
        match self {
            Self::Initiate => OssPending::Initiate,
            Self::Put => OssPending::Put,
            Self::Parts { .. } => OssPending::Parts,
            Self::Complete => OssPending::Complete,
            Self::Abort => OssPending::Abort,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct OssView {
    pub id: String,
    pub source: PathBuf,
    pub status: OssStatus,
    pub bytes: u64,
    pub total: u64,
    pub pending: Option<OssPending>,
    pub requested: Option<OssControl>,
    pub part_size: usize,
    pub concurrency: usize,
    pub confirmed_parts: usize,
    pub total_parts: u32,
    pub crc64_verified: bool,
    pub content_verified: bool,
    pub error: Option<String>,
}
#[derive(Clone, Default)]
pub struct OssRunOptions {
    pub chunk_budget: Option<usize>,
    pub stop: Arc<AtomicBool>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Part {
    pub number: u32,
    pub bytes: u64,
    pub md5: String,
    pub confirmed: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Job {
    pub id: String,
    pub source: Source,
    pub allocation: OssAllocation,
    pub crc64: u64,
    pub part_size: usize,
    pub concurrency: usize,
    pub status: OssStatus,
    pub upload_id: Option<String>,
    pub parts: BTreeMap<u32, Part>,
    pub pending: Option<Pending>,
    pub completion: Option<Completion>,
    pub error: Option<String>,
}
pub(super) fn valid_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Err(Error::InvalidInput("invalid OSS task ID"));
    }
    Ok(())
}
pub(super) fn configuration(part_size: usize, concurrency: usize) -> Result<()> {
    if !(100 * 1024..=64 * 1024 * 1024).contains(&part_size) || !(1..=3).contains(&concurrency) {
        return Err(Error::InvalidInput(
            "OSS parts require 100KiB..64MiB and concurrency 1..3",
        ));
    }
    Ok(())
}
impl Job {
    pub fn count(&self) -> u32 {
        self.source.bytes.div_ceil(self.part_size as u64) as u32
    }
    pub fn part_bytes(&self, n: u32) -> u64 {
        (self.source.bytes - ((n - 1) as u64 * self.part_size as u64)).min(self.part_size as u64)
    }
    pub fn validate(&self) -> Result<()> {
        valid_id(&self.id)?;
        self.source.validate()?;
        self.allocation.validate()?;
        configuration(self.part_size, self.concurrency)?;
        if self.count() > 10000 {
            return Err(Error::State("too many OSS parts"));
        }
        if let Some(id) = &self.upload_id {
            request::valid_upload_id(id)?;
        }
        for (number, part) in &self.parts {
            if *number == 0
                || *number > self.count()
                || part.number != *number
                || part.bytes != self.part_bytes(*number)
                || request::etag(&part.md5)? != part.md5
            {
                return Err(Error::State("invalid persisted OSS part"));
            }
        }
        if let Some(Pending::Parts { numbers }) = &self.pending {
            let mut seen = std::collections::BTreeSet::new();
            if numbers.len() > self.concurrency
                || numbers
                    .iter()
                    .any(|n| !seen.insert(*n) || self.parts.get(n).is_none_or(|p| p.confirmed))
            {
                return Err(Error::State("invalid OSS pending batch"));
            }
        }
        if self.status == OssStatus::Uploaded
            && (self.pending.is_some() || self.completion.is_none())
        {
            return Err(Error::State("incomplete OSS uploaded record"));
        }
        Ok(())
    }
    pub fn view(&self, requested: Option<OssControl>) -> OssView {
        OssView {
            id: self.id.clone(),
            source: self.source.path.clone(),
            status: self.status,
            bytes: if self.status == OssStatus::Uploaded {
                self.source.bytes
            } else {
                self.parts
                    .values()
                    .filter(|p| p.confirmed)
                    .map(|p| p.bytes)
                    .sum()
            },
            total: self.source.bytes,
            pending: self.pending.as_ref().map(Pending::kind),
            requested,
            part_size: self.part_size,
            concurrency: self.concurrency,
            confirmed_parts: self.parts.values().filter(|p| p.confirmed).count(),
            total_parts: self.count(),
            crc64_verified: self.completion.as_ref().is_some_and(|c| c.crc64_verified),
            content_verified: self.completion.as_ref().is_some_and(|c| c.content_verified),
            error: self.error.clone(),
        }
    }
}
