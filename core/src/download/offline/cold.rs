//! Cold offline audio is opened without an AccountSession or an HTTP backend.
use super::{
    OfflineRecord,
    service::{KIND, path_id},
};
use crate::{
    Error, Result,
    account::profiles::ColdOfflineGrant,
    download::service::{DownloadOrigin, DownloadTask, TASKS},
    media::{
        ncm::{HeaderPolicy, NcmReader},
        open_regular,
    },
    membership::{ColdEntitlement, ColdMembership, Quality},
    playback::engine::{pinned_audio_format, pinned_ncm_format},
    storage::StateStore,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

mod flac;
pub use flac::{ColdOfflineFlacAuthority, ColdOfflineFlacReader};

/// Native handle to already-opened, verified downloaded audio. No file path or
/// account identity is exposed. A future host must stop decoding on revocation.
pub struct ColdOfflineAudio<'a> {
    grant: &'a ColdOfflineGrant,
    file: File,
    quality: Quality,
    entitlement: Option<ColdEntitlement>,
    format: &'static str,
}
impl<'a> ColdOfflineAudio<'a> {
    pub(crate) fn new(
        grant: &'a ColdOfflineGrant,
        file: File,
        quality: Quality,
        entitlement: Option<ColdEntitlement>,
        format: &'static str,
    ) -> Self {
        Self {
            grant,
            file,
            quality,
            entitlement,
            format,
        }
    }

    pub fn file(&self) -> Result<&File> {
        self.grant.validate()?;
        if let Some(entitlement) = &self.entitlement {
            self.grant.validate_entitlement(entitlement)?;
        }
        Ok(&self.file)
    }

    pub(crate) fn into_playback(self) -> Result<(File, Option<ColdEntitlement>, &'static str)> {
        self.file()?;
        Ok((self.file, self.entitlement, self.format))
    }

    pub fn quality(&self) -> Quality {
        self.quality
    }
}

const SCAN_LIMIT: usize = 256;
const PAGE_LIMIT: usize = 64;

/// Safe listing fields. Eligibility is a ledger observation, not a fresh
/// audio hash; the open path independently verifies the file.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColdOfflineAudioRow {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub actual_quality: &'static str,
}

/// Kept by the native host; never serialize a database cursor to the renderer.
pub struct ColdOfflineCursor {
    grant: uuid::Uuid,
    after: String,
}

pub struct ColdOfflineAudioPage {
    pub rows: Vec<ColdOfflineAudioRow>,
    pub next: Option<ColdOfflineCursor>,
}

struct Eligible {
    task: DownloadTask,
    path: PathBuf,
    ledger_id: String,
    ledger: Value,
    record: OfflineRecord,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn eligible(
    store: &StateStore,
    owner: &str,
    task_id: &str,
    raw: &Value,
    include_encrypted: bool,
) -> Result<Eligible> {
    if !valid_id(task_id) {
        return Err(Error::InvalidInput(
            "invalid cold offline download identifier",
        ));
    }
    let task: DownloadTask = serde_json::from_value(raw.clone())?;
    if task.id != task_id
        || !matches!(task.origin, DownloadOrigin::Song)
        || (task.encrypted && !include_encrypted)
    {
        return Err(Error::State(
            "download is not an eligible cold offline song",
        ));
    }
    let path = task.playback_target()?.canonicalize()?;
    let ledger_id = path_id(&path)?;
    let ledger = store
        .get(owner, KIND, &ledger_id)?
        .ok_or(Error::State("download has no offline record"))?;
    let record: OfflineRecord = serde_json::from_value(ledger.clone())?;
    if record.id != ledger_id
        || record.path != path
        || record.task_id != task_id
        || record.song_id != task.song_id
        || record.encrypted != task.encrypted
        || task.output_integrity.as_ref() != Some(&record.integrity)
    {
        return Err(Error::State("cold offline download ledger mismatch"));
    }
    record.integrity.validate()?;
    Ok(Eligible {
        task,
        path,
        ledger_id,
        ledger,
        record,
    })
}

fn unchanged(
    store: &StateStore,
    owner: &str,
    task_id: &str,
    raw: &Value,
    ledger_id: &str,
    ledger: &Value,
) -> Result<bool> {
    store.transaction(|tx| {
        Ok(tx.get(owner, TASKS, task_id)?.as_ref() == Some(raw)
            && tx.get(owner, KIND, ledger_id)?.as_ref() == Some(ledger))
    })
}

fn text(value: &Value) -> String {
    value
        .as_str()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect()
}

fn check_format(file: &mut File, encrypted: bool) -> Result<&'static str> {
    if encrypted {
        let mut reader = NcmReader::new(file, HeaderPolicy::Strict)?;
        pinned_ncm_format(&mut reader)
    } else {
        pinned_audio_format(file)
    }
}

pub(crate) fn list_audio(
    grant: uuid::Uuid,
    store: &StateStore,
    owner: &str,
    after: Option<&ColdOfflineCursor>,
    limit: usize,
    membership: Option<&ColdMembership>,
) -> Result<ColdOfflineAudioPage> {
    if !(1..=PAGE_LIMIT).contains(&limit) {
        return Err(Error::InvalidInput("invalid cold offline page size"));
    }
    if after.is_some_and(|cursor| cursor.grant != grant) {
        return Err(Error::StaleResponse);
    }
    let records = store.transaction(|tx| {
        tx.values_page(
            owner,
            TASKS,
            after.map(|cursor| cursor.after.as_str()),
            SCAN_LIMIT + 1,
        )
    })?;
    let mut records = records.into_iter().peekable();
    let mut last = None;
    let mut rows = Vec::with_capacity(limit);
    for _ in 0..SCAN_LIMIT {
        let Some((id, raw)) = records.next() else {
            break;
        };
        let item = eligible(store, owner, &id, &raw, membership.is_some());
        last = Some(id.clone());
        let item = match item {
            Ok(item) => item,
            Err(
                Error::State(_)
                | Error::InvalidInput(_)
                | Error::Io(_)
                | Error::Json(_)
                | Error::MutationBusy,
            ) => continue,
            Err(error) => return Err(error),
        };
        let entitlement = if item.record.encrypted {
            let membership =
                membership.ok_or(Error::State("encrypted offline rights unavailable"))?;
            match membership.entitlement(store, owner, &item.record.privilege, item.record.quality)
            {
                Ok(entitlement) => Some(entitlement),
                Err(
                    Error::State(_) | Error::Response(_) | Error::InvalidInput(_) | Error::Json(_),
                ) => continue,
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        if open_regular(&item.path)
            .and_then(|mut file| check_format(&mut file, item.record.encrypted))
            .is_err()
            || !unchanged(store, owner, &id, &raw, &item.ledger_id, &item.ledger)?
            || entitlement.as_ref().is_some_and(|rights| !rights.valid())
        {
            continue;
        }
        let song = item.task.song.as_ref().unwrap_or(&Value::Null);
        let artist = song
            .get("ar")
            .or_else(|| song.get("artists"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(8)
            .map(|artist| text(&artist["name"]))
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>()
            .join(" / ");
        rows.push(ColdOfflineAudioRow {
            id,
            title: text(&song["name"]),
            artist,
            actual_quality: item.record.quality.as_str(),
        });
        if rows.len() == limit {
            break;
        }
    }
    let next = records
        .peek()
        .and(last)
        .map(|after| ColdOfflineCursor { grant, after });
    Ok(ColdOfflineAudioPage { rows, next })
}

pub(crate) fn open_audio(
    store: &StateStore,
    owner: &str,
    task_id: &str,
    membership: Option<&ColdMembership>,
) -> Result<(File, Quality, Option<ColdEntitlement>, &'static str)> {
    if !valid_id(task_id) {
        return Err(Error::InvalidInput(
            "invalid cold offline download identifier",
        ));
    }
    let raw = store
        .get(owner, TASKS, task_id)?
        .ok_or(Error::State("download not found"))?;
    let Eligible {
        path,
        ledger_id,
        ledger,
        record,
        ..
    } = eligible(store, owner, task_id, &raw, membership.is_some())?;
    let entitlement = if record.encrypted {
        Some(
            membership
                .ok_or(Error::State("encrypted offline rights unavailable"))?
                .entitlement(store, owner, &record.privilege, record.quality)?,
        )
    } else {
        None
    };
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
    let format = check_format(&mut file, record.encrypted)?;
    file.seek(SeekFrom::Start(0))?;
    if let Some(entitlement) = &entitlement {
        entitlement.validate(store, owner)?;
    }
    if !unchanged(store, owner, task_id, &raw, &ledger_id, &ledger)? {
        return Err(Error::StaleResponse);
    }
    Ok((file, record.quality, entitlement, format))
}
