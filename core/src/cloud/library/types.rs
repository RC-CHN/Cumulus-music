use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// Native records: `song` is never passed directly across IPC.
#[derive(Clone, Serialize, Deserialize)]
pub struct CloudEntry {
    pub song_id: String,
    pub file_name: String,
    pub file_size: u64,
    pub added_at: u64,
    pub bitrate: u64,
    pub song: Value,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct CloudSnapshot {
    pub revision: String,
    pub updated_ms: u64,
    pub size: Option<u64>,
    pub max_size: Option<u64>,
    pub items: Vec<CloudEntry>,
}
impl CloudSnapshot {
    pub(super) fn new(
        items: Vec<CloudEntry>,
        size: Option<u64>,
        max_size: Option<u64>,
    ) -> Result<Self> {
        let mut canonical = json!([items, size, max_size]);
        // The server rotates p3/p4 image hosts between identical list reads.
        // Artwork transport addresses do not change the selected cloud songs.
        // Retain them in the snapshot; exclude them only from its revision.
        for item in canonical[0].as_array_mut().expect("items are an array") {
            if let Some(album) = item["song"]["al"].as_object_mut() {
                album.remove("picUrl");
            }
        }
        canonical.sort_all_objects();
        let revision = hex::encode(Sha256::digest(serde_json::to_vec(&canonical)?));
        Ok(Self {
            revision,
            updated_ms: now_ms()?,
            items,
            size,
            max_size,
        })
    }
    pub(super) fn validate(&self) -> Result<()> {
        if self.items.len() > 200_000 || self.revision.len() != 64 {
            return Err(Error::State("invalid cloud snapshot"));
        }
        let mut ids = BTreeSet::new();
        for item in &self.items {
            crate::protocol::api::identifier(&json!(item.song_id))?;
            if !ids.insert(&item.song_id) || !item.song.is_object() {
                return Err(Error::State("invalid cloud entry"));
            }
        }
        if Self::new(self.items.clone(), self.size, self.max_size)?.revision != self.revision {
            // Accept already persisted v1 snapshots. The next successful
            // refresh stores the stable revision without discarding the cache.
            let mut legacy = json!([self.items, self.size, self.max_size]);
            legacy.sort_all_objects();
            if hex::encode(Sha256::digest(serde_json::to_vec(&legacy)?)) != self.revision {
                return Err(Error::State("cloud snapshot revision mismatch"));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletePhase {
    Sending,
    Verifying,
    Uncertain,
    Confirmed,
    Partial,
    Rejected,
}
impl DeletePhase {
    pub fn pending(self) -> bool {
        matches!(self, Self::Sending | Self::Verifying | Self::Uncertain)
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum BatchPhase {
    Queued,
    Sending,
    Acknowledged,
    Rejected,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct DeleteBatch {
    pub ids: Vec<String>,
    pub phase: BatchPhase,
    pub succeeded: Vec<String>,
    pub failed: Vec<String>,
    pub code: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct DeleteOperation {
    pub id: String,
    pub created_ms: u64,
    pub phase: DeletePhase,
    pub ids: Vec<String>,
    pub removed: Vec<String>,
    pub remaining: Vec<String>,
    pub before_revision: String,
    pub after_revision: Option<String>,
    pub(super) batches: Vec<DeleteBatch>,
}
impl DeleteOperation {
    pub(super) fn validate(&self) -> Result<()> {
        valid_id(&self.id)?;
        let ids = checked_ids(&self.ids)?;
        let batch_ids = self
            .batches
            .iter()
            .flat_map(|b| b.ids.clone())
            .collect::<Vec<_>>();
        if batch_ids != ids
            || self
                .batches
                .iter()
                .any(|b| b.ids.is_empty() || b.ids.len() > 1000)
        {
            return Err(Error::State("cloud deletion journal mismatch"));
        }
        for subset in [&self.removed, &self.remaining] {
            let mut seen = BTreeSet::new();
            if subset
                .iter()
                .any(|id| !ids.contains(id) || !seen.insert(id))
            {
                return Err(Error::State("invalid cloud deletion observation"));
            }
        }
        Ok(())
    }
}
pub(super) fn checked_ids(ids: &[String]) -> Result<Vec<String>> {
    if ids.is_empty() || ids.len() > 5000 {
        return Err(Error::InvalidInput("invalid cloud selection size"));
    }
    let mut seen = BTreeSet::new();
    ids.iter()
        .map(|id| {
            let id = crate::protocol::api::identifier(&json!(id))?;
            if !seen.insert(id.clone()) {
                return Err(Error::InvalidInput("duplicate cloud selection"));
            }
            Ok(id)
        })
        .collect()
}
pub(super) fn valid_id(id: &str) -> Result<()> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::InvalidInput("invalid cloud operation ID"));
    }
    Ok(())
}
pub(super) fn now_ms() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::State("clock before epoch"))?
        .as_millis()
        .try_into()
        .map_err(|_| Error::State("cloud clock overflow"))
}
