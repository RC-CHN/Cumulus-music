use super::{
    control::Control,
    files,
    manager::{CONTROL, KIND},
    *,
};
use crate::{
    Error, Result, account::SessionToken, download::offline::PublishedDownload,
    media::integrity::inspect_file,
};
use std::fs;
impl DownloadService {
    pub(super) async fn publish(
        &self,
        mut task: DownloadTask,
        gate: Control,
    ) -> Result<DownloadTask> {
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            service.guard(&gate.token)?;
            gate.require_alive()?;
            let artifact = task
                .prepared
                .as_ref()
                .ok_or(Error::State("missing prepared output"))?;
            artifact.verify(&task)?;
            let target = files::target(&task)?.to_path_buf();
            let linked = files::published(&task)?;
            if !linked {
                gate.require()?;
            }
            // Publish is short, and account selection cannot cross this link.
            service.session.with_token(&gate.token, |_| {
                gate.require_alive()?;
                if !linked
                    && (gate.stop.load(std::sync::atomic::Ordering::SeqCst)
                        || gate.action()?.is_some())
                {
                    return Err(Error::Cancelled);
                }
                files::work(&task)?;
                files::target(&task)?;
                if !files::published(&task)? {
                    files::absent(&target)?;
                    fs::hard_link(&artifact.path, &target)?;
                }
                files::sync(target.parent().expect("validated"))
            })?;
            let integrity = artifact
                .integrity
                .clone()
                .ok_or(Error::State("prepared digest missing"))?;
            let published = PublishedDownload {
                task_id: task.id.clone(),
                song_id: task.song_id.clone(),
                requested_quality: task.options.quality,
                target,
                song: task
                    .song
                    .clone()
                    .ok_or(Error::State("missing download metadata"))?,
                resource: task
                    .resource
                    .clone()
                    .ok_or(Error::State("missing download authorization"))?,
                encrypted: task.encrypted,
                integrity: integrity.clone(),
            };
            let mut committed = task.clone();
            committed.status = DownloadStatus::Complete;
            if committed.completed_ms.is_none() {
                committed.completed_ms = Some(
                    u64::try_from(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_err(|_| Error::State("invalid completion clock"))?
                            .as_millis(),
                    )
                    .map_err(|_| Error::State("completion clock overflow"))?,
                );
            }
            committed.phase = DownloadPhase::Complete;
            committed.output_integrity = Some(integrity);
            committed.cleanup_pending = true;
            committed.last_error = None;
            committed.service_code = None;
            service
                .offline
                .register_with(&gate.token, &published, |tx, _| {
                    // Once linked, pause/cancel cannot retract an output. Account
                    // generation and async lifetime still gate the atomic commit.
                    gate.transaction_gate(tx, true)?;
                    if !files::published(&task)? {
                        return Err(Error::State("published output changed identity"));
                    }
                    tx.put(
                        &service.owner,
                        KIND,
                        &task.id,
                        &serde_json::to_value(&committed)?,
                    )?;
                    tx.delete(&service.owner, CONTROL, &task.id)
                })?;
            task = committed;
            Ok(task)
        })
        .await
        .map_err(|_| Error::State("download publisher stopped"))?
    }
    pub(super) async fn verify_complete(
        &self,
        task: DownloadTask,
        gate: Control,
    ) -> Result<DownloadTask> {
        tokio::task::spawn_blocking(move || {
            gate.service.guard(&gate.token)?;
            gate.require_alive()?;
            if !files::published(&task)?
                || task.output_integrity.as_ref() != Some(&inspect_file(files::target(&task)?)?)
                || !files::published(&task)?
            {
                return Err(Error::Media("completed download changed"));
            }
            let record = gate
                .service
                .offline
                .record(&gate.token, files::target(&task)?)?
                .ok_or(Error::State("completed download has no offline record"))?;
            if record.task_id != task.id || Some(record.integrity) != task.output_integrity {
                return Err(Error::State("offline record differs from completed task"));
            }
            Ok(task)
        })
        .await
        .map_err(|_| Error::State("download verification worker stopped"))?
    }
    pub(super) fn cancel_task(&self, token: &SessionToken, task: &mut DownloadTask) -> Result<()> {
        self.guard(token)?;
        if files::published(task)? || task.status == DownloadStatus::Complete {
            return Err(Error::State("published output cannot be cancelled"));
        }
        task.status = DownloadStatus::Cancelling;
        self.save_current(token, task)?;
        task.cleanup_pending = self.cleanup_files(task).is_err();
        task.status = DownloadStatus::Cancelled;
        self.session.with_token(token, |_| {
            self.store.transaction(|tx| {
                tx.put(&self.owner, KIND, &task.id, &serde_json::to_value(&*task)?)?;
                tx.delete(&self.owner, CONTROL, &task.id)
            })
        })
    }
    pub(super) fn cleanup(&self, token: &SessionToken, task: &mut DownloadTask) -> Result<()> {
        task.cleanup_pending = self.cleanup_files(task).is_err();
        self.save_current(token, task)
    }
    fn cleanup_files(&self, task: &DownloadTask) -> Result<()> {
        files::directory(&task.root, &task.root_identity)?;
        match fs::symlink_metadata(&task.work) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
            Ok(_) => (),
        }
        files::work(task)?;
        if self
            .store
            .get(&self.owner, "media_transfers_v1", &task.id)?
            .is_some()
        {
            self.transfer
                .discard_intermediate(&task.id, &task.work.join("raw"))?;
        }
        for artifact in [&task.prepared, &task.cover].into_iter().flatten() {
            artifact.remove(task)?;
        }
        if task.status == DownloadStatus::Complete
            && task.options.lyrics
            && matches!(
                task.lyrics_status,
                LyricsStatus::Pending | LyricsStatus::Prepared | LyricsStatus::Error
            )
        {
            return Err(Error::State("lyric sidecar awaits completion"));
        }
        if let Some(artifact) = &task.lyric {
            artifact.remove(task)?;
        }
        fs::remove_dir(&task.work)?;
        files::sync(&task.root)
    }
}
