//! One guarded, ordered admission of new catalog song tasks.
use super::{
    DownloadDestination, DownloadOptions, DownloadOrigin, DownloadService, files,
    manager::{KIND, TaskDestination, creation_clock},
};
use crate::{
    Error, Result,
    account::resources::{AccountResources, ResourceKind},
    download::directory::{DirectorySettings, gate as directory_gate},
    playlist::GroupDownloadPlaylistProof,
    protocol::api::identifier,
    storage::{OperationLock, PlaybackDownloadKind},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
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
}

impl DownloadService {
    pub(crate) fn create_guarded_song_batch(
        &self,
        ids: &[String],
        receipt: &str,
        admission: SongBatchAdmission,
        options: DownloadOptions,
        permission: &dyn Fn() -> Result<OperationLock>,
        cancelled: &AtomicBool,
    ) -> Result<Vec<String>> {
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
        if ids.is_empty()
            || ids.len() > 100
            || ids.iter().collect::<HashSet<_>>().len() != ids.len()
        {
            return Err(Error::InvalidInput(
                "invalid download batch size or duplicate songs",
            ));
        }
        let created_ms = creation_clock()?;
        // Both the worker and the durable list use (created_ms, id). Keep
        // the real common timestamp and assign ascending random IDs to rows.
        let mut task_ids: Vec<_> = ids
            .iter()
            .map(|_| uuid::Uuid::new_v4().simple().to_string())
            .collect();
        task_ids.sort_unstable();
        let staged = ids
            .iter()
            .zip(&task_ids)
            .map(|(song, task)| {
                self.stage_task(
                    task.clone(),
                    created_ms,
                    identifier(&json!(song))?,
                    &destination,
                    options.clone(),
                    (DownloadOrigin::Song, None),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        files::sync(&destination.root)?;
        files::directory(&destination.root, &destination.identity)?;
        // This check may consult account and local state. Never call it
        // while holding a StateStore transaction or the account mutex.
        let _local = permission()?;
        check_cancelled()?;
        self.session.with_token(&token, |owner| {
            if owner != self.owner {
                return Err(Error::StaleResponse);
            }
            self.store.transaction(|tx| {
                match &admission {
                    SongBatchAdmission::LikedAnchor(anchor) => {
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
                        if tx
                            .playback_download_rows(owner, anchor, PlaybackDownloadKind::Song)?
                            .iter()
                            .any(|row| row.status == "complete")
                        {
                            return Err(Error::State("song batch anchor is already downloaded"));
                        }
                    }
                    SongBatchAdmission::Playlist(proof) => proof.verify_in(tx, owner)?,
                }
                for task in &staged {
                    let task = task.task();
                    if tx
                        .playback_download_rows(owner, &task.song_id, PlaybackDownloadKind::Song)?
                        .iter()
                        .any(|row| row.status != "cancelled")
                        || tx.get(owner, KIND, &task.id)?.is_some()
                    {
                        return Err(Error::State("song already has a download"));
                    }
                    tx.put(owner, KIND, &task.id, &serde_json::to_value(task)?)?;
                }
                tx.put(
                    owner,
                    BATCH,
                    LAST,
                    &serde_json::to_value(Commit {
                        receipt: receipt.into(),
                        tasks: task_ids.clone(),
                    })?,
                )?;
                // No account/store lookups can be repeated from inside this
                // transaction, but a dropped caller still revokes precommit.
                check_cancelled()
            })
        })?;
        for task in staged {
            task.commit();
        }
        Ok(task_ids)
    }
}
