//! Classify the absent leaf of a published public-catalog download.
use super::{DownloadOrigin, DownloadPhase, DownloadService, DownloadTask, files, manager::KIND};
use crate::{Error, Result, account::SessionToken, download::offline::OfflineRecord};
use serde_json::Value;
use std::{fs, io::ErrorKind, path::Path};

#[derive(PartialEq)]
pub(crate) struct MissingCatalogDownload {
    pub song: Value,
    ledger: OfflineRecord,
    root: files::Identity,
    parent: files::Identity,
}

impl DownloadService {
    pub(crate) fn missing_catalog_playback(
        &self,
        token: &SessionToken,
        task_id: &str,
        path: &Path,
        song_id: &str,
    ) -> Result<Option<MissingCatalogDownload>> {
        self.guard(token)?;
        let _task = self.lock(task_id)?;
        let Some(raw) = self.store.get(&self.owner, KIND, task_id)? else {
            return Ok(None);
        };
        // Older rows deserialize a missing origin as Song. This fallback
        // requires positive provenance before sharing any ID with the API.
        if raw["origin"]["kind"].as_str() != Some("song") {
            return Ok(None);
        }
        let task: DownloadTask = serde_json::from_value(raw)?;
        let Some((song, integrity)) = task.song.as_ref().zip(task.output_integrity.as_ref()) else {
            return Ok(None);
        };
        if task.id != task_id
            || task.song_id != song_id
            || crate::protocol::api::identifier(&song["id"])
                .ok()
                .as_deref()
                != Some(song_id)
            || task.origin != DownloadOrigin::Song
            || task.phase != DownloadPhase::Complete
            || task.encrypted
            || task.cleanup_pending
            || task.playback_target().ok() != Some(path)
        {
            return Ok(None);
        }
        integrity.validate()?;
        let managed = files::target(&task)?;
        let parent = managed
            .parent()
            .ok_or(Error::State("download target has no parent"))?;
        if task.root.canonicalize()? != task.root || parent.canonicalize()? != parent {
            return Ok(None);
        }
        let Some(ledger) = self.offline.record_at_verified_path(token, managed)? else {
            return Ok(None);
        };
        if ledger.path != managed
            || ledger.task_id != task_id
            || ledger.song_id != song_id
            || ledger.encrypted
            || &ledger.integrity != integrity
        {
            return Ok(None);
        }
        match fs::symlink_metadata(managed) {
            // Unlike canonicalize or metadata, a dangling link is present.
            Err(error) if error.kind() == ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
            Ok(_) => return Ok(None),
        }
        // ENOENT also covers absent or moved parents. Reconfirm the pinned
        // managed directories after observing the leaf to distinguish them.
        files::target(&task)?;
        if task.root.canonicalize()? != task.root || parent.canonicalize()? != parent {
            return Ok(None);
        }
        self.guard(token)?;
        Ok(Some(MissingCatalogDownload {
            song: song.clone(),
            ledger,
            root: task.root_identity,
            parent: task
                .target_parent
                .ok_or(Error::State("missing target directory identity"))?,
        }))
    }
}
