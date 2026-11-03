//! Cold offline audio is opened without an AccountSession or an HTTP backend.
use super::{
    OfflineRecord,
    service::{KIND, path_id},
};
use crate::{
    Error, Result,
    account::profiles::ColdOfflineGrant,
    download::service::{DownloadOrigin, DownloadTask, TASKS},
    media::open_regular,
    membership::Quality,
    storage::StateStore,
};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

/// Native handle to already-opened, verified plain audio. No file path or
/// account identity is exposed. A future host must stop decoding on revocation.
pub struct ColdOfflineAudio<'a> {
    grant: &'a ColdOfflineGrant,
    file: File,
    quality: Quality,
}
impl<'a> ColdOfflineAudio<'a> {
    pub(crate) fn new(grant: &'a ColdOfflineGrant, file: File, quality: Quality) -> Self {
        Self {
            grant,
            file,
            quality,
        }
    }

    pub fn file(&self) -> Result<&File> {
        self.grant.validate()?;
        Ok(&self.file)
    }

    pub fn quality(&self) -> Quality {
        self.quality
    }
}

pub(crate) fn open_plain_audio(
    store: &StateStore,
    owner: &str,
    task_id: &str,
) -> Result<(File, Quality)> {
    if task_id.is_empty()
        || task_id.len() > 256
        || !task_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(Error::InvalidInput(
            "invalid cold offline download identifier",
        ));
    }
    let raw = store
        .get(owner, TASKS, task_id)?
        .ok_or(Error::State("download not found"))?;
    let task: DownloadTask = serde_json::from_value(raw.clone())?;
    if task.id != task_id || !matches!(task.origin, DownloadOrigin::Song) || task.encrypted {
        return Err(Error::State(
            "cold offline mode only supports plain downloaded songs",
        ));
    }
    let path = task.playback_target()?.canonicalize()?;
    let id = path_id(&path)?;
    let ledger = store
        .get(owner, KIND, &id)?
        .ok_or(Error::State("download has no offline record"))?;
    let record: OfflineRecord = serde_json::from_value(ledger.clone())?;
    if record.id != id
        || record.path != path
        || record.task_id != task_id
        || record.song_id != task.song_id
        || record.encrypted
        || task.output_integrity.as_ref() != Some(&record.integrity)
    {
        return Err(Error::State("cold offline download ledger mismatch"));
    }
    record.integrity.validate()?;
    let mut file = open_regular(&path)?;
    let before = file.metadata()?;
    if before.len() != record.integrity.bytes {
        return Err(Error::Media("cold offline file changed"));
    }
    let mut sha = Sha256::new();
    let bytes = std::io::copy(
        &mut (&mut file).take(record.integrity.bytes.saturating_add(1)),
        &mut sha,
    )?;
    let after = file.metadata()?;
    if bytes != record.integrity.bytes
        || hex::encode(sha.finalize()) != record.integrity.sha256
        || before.len() != after.len()
        || before.modified()? != after.modified()?
    {
        return Err(Error::Media("cold offline file changed"));
    }
    file.seek(SeekFrom::Start(0))?;
    store.transaction(|tx| {
        if tx.get(owner, TASKS, task_id)?.as_ref() != Some(&raw)
            || tx.get(owner, KIND, &id)?.as_ref() != Some(&ledger)
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    })?;
    Ok((file, record.quality))
}
