use crate::{
    Error, Result,
    media::integrity::FileIntegrity,
    membership::{OfflineAccess, Quality},
    protocol::api::identifier,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

/// Input from the download publisher. Register only after final media exists;
/// the registry independently verifies this expected digest before committing.
pub struct PublishedDownload {
    pub task_id: String,
    pub song_id: String,
    pub requested_quality: Quality,
    pub target: PathBuf,
    pub song: Value,
    pub resource: Value,
    pub encrypted: bool,
    pub integrity: FileIntegrity,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OfflineRecord {
    pub id: String,
    pub revision: String,
    pub path: PathBuf,
    pub task_id: String,
    pub song_id: String,
    pub quality: Quality,
    pub privilege: Value,
    pub encrypted: bool,
    pub integrity: FileIntegrity,
}
#[derive(Clone, Debug, Serialize)]
pub struct OfflineCheck {
    pub access: OfflineAccess,
    pub record: OfflineRecord,
    pub integrity_verified: bool,
}

impl PublishedDownload {
    pub(super) fn validate(&self) -> Result<()> {
        self.integrity.validate()?;
        if self.task_id.is_empty() || self.task_id.len() > 256 {
            return Err(Error::InvalidInput("invalid download task ID"));
        }
        let id = identifier(&Value::String(self.song_id.clone()))?;
        if id != self.song_id || identifier(&self.song["id"])? != id || !self.resource.is_object() {
            return Err(Error::InvalidInput(
                "download song/resource identity mismatch",
            ));
        }
        if let Some(other) = self.resource.get("id").filter(|v| !v.is_null())
            && identifier(other)? != id
        {
            return Err(Error::InvalidInput(
                "download resource belongs to another song",
            ));
        }
        Ok(())
    }
    pub(super) fn record(&self, path: PathBuf, id: String) -> Result<OfflineRecord> {
        let mut privilege = match self.song.get("privilege") {
            None | Some(Value::Null) => serde_json::Map::new(),
            Some(Value::Object(p)) => p.clone(),
            _ => return Err(Error::InvalidInput("song privilege must be an object")),
        };
        for field in ["fee", "payed"] {
            if let Some(value) = self.resource.get(field).filter(|v| !v.is_null()) {
                privilege.insert(field.into(), value.clone());
            }
        }
        if !privilege.contains_key("flag") {
            privilege.insert(
                "flag".into(),
                self.resource.get("flag").cloned().unwrap_or(Value::from(0)),
            );
        }
        let quality = match self.resource.get("level") {
            None => self.requested_quality,
            Some(Value::String(level)) => Quality::parse(level)?,
            _ => return Err(Error::InvalidInput("invalid actual download quality")),
        };
        Ok(OfflineRecord {
            id,
            revision: uuid::Uuid::new_v4().to_string(),
            path,
            task_id: self.task_id.clone(),
            song_id: self.song_id.clone(),
            quality,
            privilege: privilege.into(),
            encrypted: self.encrypted,
            integrity: self.integrity.clone(),
        })
    }
}
