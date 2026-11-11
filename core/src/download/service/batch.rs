//! One guarded, ordered admission of catalog song tasks.
use super::{
    DownloadAction, DownloadDestination, DownloadOptions, DownloadOrigin, DownloadPhase,
    DownloadService, DownloadStatus, DownloadTask,
    admission::catalog_admission_ids,
    control::{Control, ObservedControl},
    dispatch::Dispatch,
    files,
    manager::{CONTROL, KIND, StagedTask, TaskDestination, creation_clock},
};
use crate::{
    Error, Result,
    account::resources::{AccountResources, ResourceKind},
    download::{
        directory::{DirectorySettings, gate as directory_gate},
        transfer::{TRANSFERS, TransferJob, TransferStatus},
    },
    history::{HistoryKind, HistoryService},
    playlist::GroupDownloadPlaylistProof,
    protocol::api::identifier,
    storage::{OperationLock, PlaybackDownloadKind, StateTransaction},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const BATCH: &str = "download_song_batch_v1";
const LAST: &str = "last";

#[derive(Deserialize, Serialize)]
struct Commit {
    receipt: String,
    tasks: Vec<String>,
}

pub(crate) enum SongBatchAdmission {
    LikedAnchor(String),
    Playlist(GroupDownloadPlaylistProof),
    SearchPlaylist {
        proof: GroupDownloadPlaylistProof,
        anchor: String,
    },
    HistoryLikedAnchor {
        anchor: String,
        source: HistoryBatchSource,
    },
    HistoryPlaylist {
        proof: GroupDownloadPlaylistProof,
        source: HistoryBatchSource,
    },
}

pub(crate) struct HistoryBatchSource {
    pub revision: String,
    pub offline: BTreeMap<String, String>,
}

impl HistoryBatchSource {
    fn verify_in(&self, tx: &StateTransaction<'_>, owner: &str) -> Result<()> {
        if HistoryService::snapshot_in(tx, owner, HistoryKind::Song)?.revision != self.revision {
            return Err(Error::QueueChanged);
        }
        for (song, task_id) in &self.offline {
            let task: DownloadTask =
                serde_json::from_value(tx.get(owner, KIND, task_id)?.ok_or(Error::QueueChanged)?)?;
            if task.id != *task_id || !task.matches_completed_history_song(song) {
                return Err(Error::QueueChanged);
            }
        }
        Ok(())
    }
}

impl SongBatchAdmission {
    pub(crate) fn playlist_proof(&self) -> Option<&GroupDownloadPlaylistProof> {
        match self {
            Self::Playlist(proof)
            | Self::SearchPlaylist { proof, .. }
            | Self::HistoryPlaylist { proof, .. } => Some(proof),
            _ => None,
        }
    }

    fn history_source(&self) -> Option<&HistoryBatchSource> {
        match self {
            Self::HistoryLikedAnchor { source, .. } | Self::HistoryPlaylist { source, .. } => {
                Some(source)
            }
            _ => None,
        }
    }
}

pub(crate) struct GuardedSongBatch {
    pub receipt: String,
    pub admission: SongBatchAdmission,
    pub songs: Arc<[String]>,
    pub options: DownloadOptions,
    pub existing: Arc<BTreeMap<String, ExistingSongAdmission>>,
}

/// Captured native row, including its exact raw and control generation. Never
/// serialize this to a caller: the old records can contain authorized URLs.
pub(crate) struct ExistingSongAdmission {
    id: String,
    row: Value,
    raw: Option<Value>,
    control: ObservedControl,
    options: DownloadOptions,
    bound_prefix: bool,
}

impl ExistingSongAdmission {
    pub(crate) fn options(&self) -> &DownloadOptions {
        &self.options
    }

    pub(crate) fn has_bound_prefix(&self) -> bool {
        self.bound_prefix
    }

    pub(crate) fn observe(
        tx: &StateTransaction<'_>,
        owner: &str,
        song: &str,
        id: &str,
    ) -> Result<Option<Self>> {
        let Some(row) = tx.get(owner, KIND, id)? else {
            return Err(Error::StaleResponse);
        };
        let task: DownloadTask = serde_json::from_value(row.clone())?;
        let control = Control::observe(tx, owner, id)?;
        if task.id != id
            || task.song_id != song
            || task.origin != DownloadOrigin::Song
            || !matches!(task.status, DownloadStatus::Paused | DownloadStatus::Error)
            || !matches!(
                task.phase,
                DownloadPhase::Authorize | DownloadPhase::Transfer
            )
            || task.file_operation.is_some()
            || task.needs_transfer_restart()
            || task.prepared.is_some()
            || task.raw_integrity.is_some()
            || task.output_integrity.is_some()
            || task.cleanup_pending
            || task.completed_ms.is_some()
            || control.action == Some(DownloadAction::Cancel)
        {
            return Ok(None);
        }
        let raw = tx.get(owner, TRANSFERS, id)?;
        let bound_prefix = if let Some(value) = &raw {
            let job: TransferJob = serde_json::from_value(value.clone())?;
            if job.id != id
                || job.resource_id.as_deref() != Some(song)
                || job.target != task.work.join("raw")
                || !matches!(
                    job.status,
                    TransferStatus::Queued | TransferStatus::Paused | TransferStatus::Error
                )
                || job.cleanup_pending
                || job.integrity.is_some()
                || job.last_error.as_deref().is_some_and(|error| {
                    error
                        == Error::Media(crate::download::transfer::FINAL_INTEGRITY_MISMATCH)
                            .to_string()
                })
            {
                return Ok(None);
            }
            job.bytes == 0
                || (job.resource.size.is_some_and(|size| size > 0)
                    && job.resource.md5.as_ref().is_some_and(|md5| !md5.is_empty()))
        } else if task.bytes != 0 {
            return Ok(None);
        } else {
            files::absent(&task.work.join("raw")).is_ok()
        };
        Ok(Some(Self {
            id: id.into(),
            row,
            raw,
            control,
            options: task.options,
            bound_prefix,
        }))
    }

    pub(crate) fn check(&self, tx: &StateTransaction<'_>, owner: &str) -> Result<()> {
        if tx.get(owner, KIND, &self.id)?.as_ref() != Some(&self.row)
            || tx.get(owner, TRANSFERS, &self.id)? != self.raw
            || Control::observe(tx, owner, &self.id)? != self.control
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    pub(super) fn stage(&self, service: &DownloadService) -> Result<HeldSongAdmission> {
        let task_lock = service.lock(&self.id)?;
        let task: DownloadTask = serde_json::from_value(self.row.clone())?;
        let raw = self.raw.clone().map(serde_json::from_value).transpose()?;
        let transfer_lock = service.transfer.hold_quiescent(&task.id, raw.as_ref())?;
        files::work(&task)?;
        if raw.is_none() {
            files::absent(&task.work.join("raw"))?;
        }
        if task.target.is_some() {
            files::absent(files::target(&task)?)?;
        }
        Ok(HeldSongAdmission {
            task,
            _transfer: transfer_lock,
            _task: task_lock,
        })
    }
}

pub(super) struct HeldSongAdmission {
    pub(super) task: DownloadTask,
    _transfer: OperationLock,
    _task: OperationLock,
}

impl HeldSongAdmission {
    pub(super) fn readmit(&mut self, tx: &mut StateTransaction<'_>, owner: &str) -> Result<()> {
        tx.delete(owner, CONTROL, &self.task.id)?;
        self.task.status = DownloadStatus::Queued;
        self.task.last_error = None;
        self.task.service_code = None;
        Ok(())
    }
}

enum BatchTask {
    New(Box<StagedTask>),
    Existing(Box<HeldSongAdmission>),
}

impl BatchTask {
    fn task_mut(&mut self) -> &mut DownloadTask {
        match self {
            Self::New(task) => task.task_mut(),
            Self::Existing(held) => &mut held.task,
        }
    }

    fn commit(self) {
        if let Self::New(task) = self {
            task.commit();
        }
    }
}

impl DownloadService {
    pub(crate) fn create_guarded_song_batch(
        &self,
        batch: GuardedSongBatch,
        permission: &dyn Fn() -> Result<OperationLock>,
        cancelled: &AtomicBool,
    ) -> Result<Vec<String>> {
        let GuardedSongBatch {
            songs: ids,
            receipt,
            admission,
            options,
            existing,
        } = batch;
        let _directory = directory_gate::lock(&self.store, &self.owner)?;
        let token = self.session.token()?;
        self.guard(&token)?;
        let check_cancelled = || {
            if cancelled.load(Ordering::Acquire) {
                Err(Error::Cancelled)
            } else {
                Ok(())
            }
        };
        check_cancelled()?;
        permission()?;
        if let Some(saved) = self.store.get(&self.owner, BATCH, LAST)? {
            let saved: Commit = serde_json::from_value(saved)?;
            if saved.receipt == receipt {
                return Ok(saved.tasks);
            }
        }
        directory_gate::require(&self.store, &self.owner, None)?;
        let settings: DirectorySettings = serde_json::from_value(
            self.store
                .get(&self.owner, directory_gate::SETTINGS, "settings")?
                .ok_or(Error::State("download directory is not configured"))?,
        )?;
        let root = settings.root.canonicalize()?;
        if root != settings.root
            || !root.is_dir()
            || root.to_str().is_none()
            || settings.path_type > 2
            || settings.file_type > 2
        {
            return Err(Error::State("download directory is no longer usable"));
        }
        let destination = TaskDestination {
            identity: files::identity(&root)?,
            destination: DownloadDestination::Named {
                root: root.clone(),
                path_type: settings.path_type,
                file_type: settings.file_type,
            },
            root,
        };
        let max_rows = if admission.history_source().is_some() {
            crate::protocol::api::GuardedPlaylistAdd::MAX_HISTORY_SONGS
        } else {
            100
        };
        if ids.is_empty()
            || ids.len() > max_rows
            || ids.iter().collect::<HashSet<_>>().len() != ids.len()
        {
            return Err(Error::InvalidInput(
                "invalid download batch size or duplicate songs",
            ));
        }
        if existing.keys().any(|song| !ids.contains(song)) {
            return Err(Error::StaleResponse);
        }
        let created_ms = creation_clock()?;
        // Preserve the existing display order for all-new batches (same
        // creation time, ascending IDs); actual dispatch has its own key.
        let mut new_ids: Vec<_> = (0..ids.len() - existing.len())
            .map(|_| uuid::Uuid::new_v4().simple().to_string())
            .collect();
        new_ids.sort_unstable();
        let mut new_ids = new_ids.into_iter();
        let mut staged = ids
            .iter()
            .map(|song| {
                if let Some(previous) = existing.get(song) {
                    Ok(BatchTask::Existing(Box::new(previous.stage(self)?)))
                } else {
                    Ok(BatchTask::New(Box::new(self.stage_task(
                        new_ids.next().expect("staged new ID"),
                        created_ms,
                        identifier(&json!(song))?,
                        &destination,
                        options.clone(),
                        (DownloadOrigin::Song, None),
                    )?)))
                }
            })
            .collect::<Result<Vec<_>>>()?;
        files::sync(&destination.root)?;
        files::directory(&destination.root, &destination.identity)?;
        // This check may consult account and local state. Never call it
        // while holding a StateStore transaction or the account mutex.
        let _local = permission()?;
        check_cancelled()?;
        let task_ids = self.session.with_token(&token, |owner| {
            if owner != self.owner {
                return Err(Error::StaleResponse);
            }
            self.store.transaction(|tx| {
                if let Some(source) = admission.history_source() {
                    source.verify_in(tx, owner)?;
                }
                match &admission {
                    SongBatchAdmission::LikedAnchor(anchor)
                    | SongBatchAdmission::HistoryLikedAnchor { anchor, .. } => {
                        let (liked, operation) = AccountResources::membership_in(
                            tx,
                            owner,
                            ResourceKind::Liked,
                            anchor,
                        )?;
                        if liked != Some(true)
                            || operation.is_some_and(|record| record.phase.pending())
                        {
                            return Err(Error::State(
                                "SongCard batch requires a confirmed liked anchor",
                            ));
                        }
                    }
                    SongBatchAdmission::Playlist(proof) => proof.verify_in(tx, owner)?,
                    SongBatchAdmission::SearchPlaylist { proof, .. }
                    | SongBatchAdmission::HistoryPlaylist { proof, .. } => {
                        proof.verify_in(tx, owner)?
                    }
                }
                let anchor = match &admission {
                    SongBatchAdmission::LikedAnchor(anchor)
                    | SongBatchAdmission::SearchPlaylist { anchor, .. } => Some(anchor),
                    SongBatchAdmission::Playlist(_)
                    | SongBatchAdmission::HistoryLikedAnchor { .. }
                    | SongBatchAdmission::HistoryPlaylist { .. } => None,
                };
                if let Some(anchor) = anchor
                    && tx
                        .playback_download_rows(owner, anchor, PlaybackDownloadKind::Song)?
                        .iter()
                        .any(|row| row.status == "complete")
                {
                    return Err(Error::State("song batch anchor is already downloaded"));
                }
                let sequence = Dispatch::reserve(tx, owner)?;
                let mut task_ids = Vec::with_capacity(staged.len());
                for (index, staged) in staged.iter_mut().enumerate() {
                    let current = catalog_admission_ids(tx, owner, &staged.task_mut().song_id)?;
                    match staged {
                        BatchTask::Existing(held) => {
                            let [row] = current.as_slice() else {
                                return Err(Error::StaleResponse);
                            };
                            if *row != held.task.id {
                                return Err(Error::StaleResponse);
                            }
                            existing
                                .get(&held.task.song_id)
                                .ok_or(Error::StaleResponse)?
                                .check(tx, owner)?;
                            held.readmit(tx, owner)?;
                        }
                        BatchTask::New(task) => {
                            if !current.is_empty()
                                || tx.get(owner, KIND, &task.task_mut().id)?.is_some()
                            {
                                return Err(Error::State("song already has a download"));
                            }
                        }
                    }
                    let task = staged.task_mut();
                    task.dispatch = Some(Dispatch::new(sequence, index));
                    tx.put(owner, KIND, &task.id, &serde_json::to_value(&*task)?)?;
                    task_ids.push(task.id.clone());
                }
                tx.put(
                    owner,
                    BATCH,
                    LAST,
                    &serde_json::to_value(Commit {
                        receipt,
                        tasks: task_ids.clone(),
                    })?,
                )?;
                // No account/store lookups can be repeated from inside this
                // transaction, but a dropped caller still revokes precommit.
                check_cancelled()?;
                Ok(task_ids)
            })
        })?;
        for task in staged {
            task.commit();
        }
        Ok(task_ids)
    }
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
