//! File identity shared by transport and business publication journals.
use crate::Result;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, time::UNIX_EPOCH};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Identity {
    id: String,
    created: Option<String>,
}
pub(crate) fn identity(path: &Path) -> Result<Identity> {
    let id = match file_id::get_file_id(path)? {
        file_id::FileId::Inode {
            device_id,
            inode_number,
        } => format!("unix:{device_id:x}:{inode_number:x}"),
        file_id::FileId::LowRes {
            volume_serial_number,
            file_index,
        } => format!("win-low:{volume_serial_number:x}:{file_index:x}"),
        file_id::FileId::HighRes {
            volume_serial_number,
            file_id,
        } => format!("win-high:{volume_serial_number:x}:{file_id:x}"),
    };
    let created = fs::metadata(path)?
        .created()
        .ok()
        .map(|t| match t.duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_nanos().to_string(),
            Err(e) => format!("-{}", e.duration().as_nanos()),
        });
    Ok(Identity { id, created })
}
