//! Ordinary catalogue download entry points share one native song admission.
use super::{
    DownloadAction, DownloadOptions, DownloadOrigin, DownloadPhase, DownloadService,
    DownloadStatus, DownloadTask, ExistingSongAdmission, control::Control, dispatch::Dispatch,
    files, manager::KIND,
};
use crate::{
    Error, Result,
    account::SessionToken,
    download::directory::gate as directory_gate,
    library::LocalLibrary,
    media::integrity::inspect_file_until,
    storage::{PlaybackDownloadKind, StateTransaction},
};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug)]
pub(crate) enum CatalogAdmission {
    Admitted(String),
    VerifyComplete(String),
    Local,
    Waiting,
    Working,
    Cancelling,
    OptionsChanged,
    Existing,
    Conflict,
}

pub(super) fn catalog_admission_ids(
    tx: &StateTransaction<'_>,
    owner: &str,
    song: &str,
) -> Result<Vec<String>> {
    // Cancellation leaves a recovery/cleanup tombstone, not a live claim.
    Ok(tx
        .playback_download_rows(owner, song, PlaybackDownloadKind::Song)?
        .into_iter()
        .filter(|row| row.status != "cancelled")
        .map(|row| row.id)
        .collect())
}

pub(super) fn require_new_catalog_task(
    tx: &StateTransaction<'_>,
    owner: &str,
    song: &str,
) -> Result<()> {
    if !catalog_admission_ids(tx, owner, song)?.is_empty() {
        return Err(Error::State("song already has a download"));
    }
    Ok(())
}

enum Candidate {
    New,
    Existing(CatalogAdmission),
    Readmit(ExistingSongAdmission),
}

impl DownloadService {
    pub(crate) fn admit_catalog_songs(
        &self,
        token: &SessionToken,
        library: &LocalLibrary,
        ids: Vec<String>,
        mut options: DownloadOptions,
        cancelled: &AtomicBool,
    ) -> Result<Vec<(String, Result<CatalogAdmission>)>> {
        self.guard(token)?;
        let _directory = directory_gate::lock(&self.store, &self.owner)?;
        let wanted = ids.iter().map(String::as_str).collect();
        let (_local, matched) = library.hold_online_song_matches(&self.session, token, &wanted)?;
        options.normalize();
        let mut outcomes = Vec::with_capacity(ids.len());
        for song in ids {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            let result = if matched.contains(&song) {
                Ok(CatalogAdmission::Local)
            } else {
                self.admit_catalog_song_locked(token, &song, &options, cancelled)
            };
            outcomes.push((song, result));
        }
        Ok(outcomes)
    }

    fn admit_catalog_song_locked(
        &self,
        token: &SessionToken,
        song: &str,
        options: &DownloadOptions,
        cancelled: &AtomicBool,
    ) -> Result<CatalogAdmission> {
        use CatalogAdmission as A;
        let candidate = self.session.with_token(token, |owner| {
            self.store.read_snapshot(|tx| {
                let rows = catalog_admission_ids(tx, owner, song)?;
                let id = match rows.as_slice() {
                    [] => return Ok(Candidate::New),
                    [id] => id,
                    _ => return Ok(Candidate::Existing(A::Conflict)),
                };
                let task: DownloadTask =
                    serde_json::from_value(tx.get(owner, KIND, id)?.ok_or(Error::StaleResponse)?)?;
                if &task.id != id || task.song_id != song || task.origin != DownloadOrigin::Song {
                    return Err(Error::State("download admission identity mismatch"));
                }
                let control = Control::read_tx(tx, owner, id)?;
                if control == Some(DownloadAction::Cancel)
                    || task.status == DownloadStatus::Cancelling
                {
                    return Ok(Candidate::Existing(A::Cancelling));
                }
                if task.file_operation.is_some() {
                    return Ok(Candidate::Existing(A::Working));
                }
                let decision = match task.status {
                    DownloadStatus::Complete => A::VerifyComplete(task.id),
                    DownloadStatus::Queued if control.is_none() => A::Waiting,
                    DownloadStatus::Queued
                    | DownloadStatus::Authorizing
                    | DownloadStatus::Downloading
                    | DownloadStatus::Processing => A::Working,
                    DownloadStatus::Paused | DownloadStatus::Error => {
                        match ExistingSongAdmission::observe(tx, owner, song, id)? {
                            Some(existing) if existing.options() != options => A::OptionsChanged,
                            Some(existing) if existing.has_bound_prefix() => {
                                return Ok(Candidate::Readmit(existing));
                            }
                            _ if &task.options != options => A::OptionsChanged,
                            _ => A::Existing,
                        }
                    }
                    DownloadStatus::Cancelling | DownloadStatus::Cancelled => {
                        return Err(Error::StaleResponse);
                    }
                };
                Ok(Candidate::Existing(decision))
            })
        })?;
        match candidate {
            Candidate::New => self
                .create_catalog_song_locked(&json!(song), options.clone(), cancelled)
                .map(|task| A::Admitted(task.id)),
            Candidate::Existing(outcome) => Ok(outcome),
            Candidate::Readmit(previous) => {
                directory_gate::require(&self.store, &self.owner, None)?;
                let mut held = previous.stage(self)?;
                let id = held.task.id.clone();
                self.session.with_token(token, |owner| {
                    self.store.transaction(|tx| {
                        let current = catalog_admission_ids(tx, owner, song)?;
                        if !matches!(current.as_slice(), [row] if *row == id) {
                            return Err(Error::StaleResponse);
                        }
                        previous.check(tx, owner)?;
                        held.readmit(tx, owner)?;
                        held.task.dispatch = Some(Dispatch::new(Dispatch::reserve(tx, owner)?, 0));
                        tx.put(owner, KIND, &id, &serde_json::to_value(&held.task)?)?;
                        if cancelled.load(Ordering::Acquire) {
                            return Err(Error::Cancelled);
                        }
                        Ok(())
                    })
                })?;
                Ok(A::Admitted(id))
            }
        }
    }

    /// No directory/index guard is held while reading the published bytes.
    /// The existing task still excludes a second admission if proof fails.
    pub(crate) fn verify_catalog_completion(
        &self,
        token: &SessionToken,
        song: &str,
        id: &str,
        cancelled: &AtomicBool,
    ) -> Result<bool> {
        let _task = self.lock(id)?;
        let task = self.get_for(token, id)?;
        if task.song_id != song || task.origin != DownloadOrigin::Song {
            return Err(Error::StaleResponse);
        }
        if task.file_operation.is_some() || task.status != DownloadStatus::Complete {
            return Err(Error::MutationBusy);
        }
        if task.phase != DownloadPhase::Complete || !files::published(&task).unwrap_or(false) {
            return Ok(false);
        }
        let target = files::target(&task)?;
        let Some(expected) = task.output_integrity.as_ref() else {
            return Ok(false);
        };
        if !std::fs::metadata(target).is_ok_and(|metadata| metadata.len() == expected.bytes) {
            return Ok(false);
        }
        match inspect_file_until(target, || cancelled.load(Ordering::Acquire)) {
            Ok(observed) if &observed == expected => {}
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            _ => return Ok(false),
        }
        if !files::published(&task).unwrap_or(false) {
            return Ok(false);
        }
        let ledger = self.offline.record_at_verified_path(token, target)?;
        self.guard(token)?;
        Ok(ledger.is_some_and(|entry| entry.task_id == task.id && &entry.integrity == expected))
    }
}
