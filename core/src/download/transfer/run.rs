use super::{
    MediaRequest, RunOptions, TransferDirective, TransferJob, TransferManager,
    TransferStatus as Status, files, headers, manager::RunMarker,
};
use crate::{Error, Result};
use std::{
    fs::{self, File},
    io::{Seek, SeekFrom, Write},
};

impl TransferManager {
    /// Run on a background runtime: small file writes/fsync and SQLite commits
    /// are synchronous checkpoints. Network and full-file hashing are awaited.
    pub async fn run(&self, id: &str, mut options: RunOptions) -> Result<TransferJob> {
        if options.byte_budget == Some(0) {
            return Err(Error::InvalidInput("transfer byte budget must be positive"));
        }
        let _lock = self.lock(id)?;
        crate::download::directory::gate::require_transfer(&self.store, &self.owner, id)?;
        let original = options.control.clone();
        let store = self.store.clone();
        let owner = self.owner.clone();
        let managed = store
            .get(&owner, crate::download::service::TASKS, id)?
            .is_some();
        options.control = std::sync::Arc::new(move || {
            let directive = original.checkpoint()?;
            if managed && crate::download::directory::gate::active(&store, &owner)?.is_some() {
                return Ok(TransferDirective::Pause);
            }
            Ok(directive)
        });
        let mut job = self.get(id)?;
        let mut marker = RunMarker {
            manager: self,
            id,
            armed: true,
        };
        let result = self.execute(&mut job, &options).await;
        if let Err(error) = &result {
            if !matches!(
                job.status,
                Status::Complete | Status::Cancelled | Status::Cancelling | Status::Publishing
            ) {
                job.status = if matches!(error, Error::Cancelled | Error::StaleResponse) {
                    Status::Paused
                } else {
                    Status::Error
                };
                if let Ok(file) = files::open_part(&job) {
                    let _ = file.sync_all();
                    if let Ok(meta) = file.metadata() {
                        job.bytes = meta.len();
                    }
                }
            }
            job.last_error = Some(error.to_string());
            self.save(&job)?;
        }
        marker.armed = false;
        result?;
        Ok(job)
    }
    async fn execute(&self, job: &mut TransferJob, options: &RunOptions) -> Result<()> {
        match job.status {
            Status::Cancelled => return Err(Error::State("transfer was cancelled")),
            Status::Cancelling => {
                files::remove_part(job)?;
                job.status = Status::Cancelled;
                job.bytes = 0;
                self.save(job)?;
                return Ok(());
            }
            Status::Complete => return self.verify_completed(job).await,
            _ => (),
        }
        if options.control.checkpoint()? == TransferDirective::Pause {
            if job.status != Status::Publishing {
                job.status = Status::Paused;
                self.save(job)?;
            }
            return Ok(());
        }
        let mut file = files::open_part(job)?;
        if job.status == Status::Publishing {
            return self.finish(job, file, options).await;
        }
        files::ensure_unpublished(job)?;
        let mut offset = file.seek(SeekFrom::End(0))?;
        if offset > self.limit || job.resource.size.is_some_and(|s| offset > s) {
            return Err(Error::Media("partial transfer exceeds expected size"));
        }
        if offset > 0 && Some(offset) == job.resource.size {
            return self.finish(job, file, options).await;
        }
        if offset > 0 && job.etag.is_none() && job.modified.is_none() && job.resource.md5.is_none()
        {
            file.set_len(0)?;
            file.rewind()?;
            file.sync_all()?;
            offset = 0;
        }
        job.status = Status::Running;
        job.bytes = offset;
        job.last_error = None;
        self.save(job)?;
        let mut reply = self
            .backend
            .get(MediaRequest {
                url: job.resource.url.clone(),
                offset,
                if_range: job.etag.clone().or_else(|| job.modified.clone()),
            })
            .await?;
        if options.control.checkpoint()? == TransferDirective::Pause {
            return self.pause(job, &mut file);
        }
        let plan = headers::validate(&reply, job, offset, self.limit)?;
        if plan.offset != offset {
            file.set_len(0)?;
            file.rewind()?;
            file.sync_all()?;
        }
        job.bytes = plan.offset;
        job.resource.size = plan.total;
        job.etag = plan.etag;
        job.modified = plan.modified;
        // Persist validators before accepting any bytes, so aborted workers
        // can resume from actual file length even between progress commits.
        self.save(job)?;
        let mut received = 0u64;
        let mut checkpoint = 0u64;
        // The buffer lives across await points. Keep it on the heap so nested
        // business futures do not multiply 64 KiB stack frames in debug hosts.
        let mut buffer = vec![0u8; 65536];
        let mut paused = false;
        loop {
            if options.control.checkpoint()? == TransferDirective::Pause
                || options.byte_budget.is_some_and(|b| received >= b)
            {
                paused = true;
                break;
            }
            let count = options
                .byte_budget
                .map(|b| (b - received).min(buffer.len() as u64) as usize)
                .unwrap_or(buffer.len());
            let n = reply.body.read(&mut buffer[..count]).await?;
            if n > count {
                return Err(Error::Response("media backend exceeded requested buffer"));
            }
            if n == 0 {
                break;
            }
            // The account/control may change while the network read waits.
            if options.control.checkpoint()? == TransferDirective::Pause {
                paused = true;
                break;
            }
            let end = job
                .bytes
                .checked_add(n as u64)
                .ok_or(Error::Response("media byte count overflow"))?;
            if end > self.limit
                || plan.total.is_some_and(|s| end > s)
                || plan.length.is_some_and(|s| received + n as u64 > s)
            {
                return Err(Error::Response("media response exceeds declared size"));
            }
            files::owned_part(job)?;
            file.write_all(&buffer[..n])?;
            received += n as u64;
            job.bytes = end;
            if received - checkpoint >= 1024 * 1024 {
                file.sync_all()?;
                self.save(job)?;
                checkpoint = received;
            }
        }
        file.sync_all()?;
        self.save(job)?;
        if Some(job.bytes) == job.resource.size {
            return self.finish(job, file, options).await;
        }
        if paused {
            return self.pause(job, &mut file);
        }
        if plan.length.is_some_and(|s| received != s) {
            return Err(Error::Response("truncated media response"));
        }
        if job.resource.size.is_some_and(|s| job.bytes < s) {
            return self.pause(job, &mut file);
        }
        self.finish(job, file, options).await
    }
    fn pause(&self, job: &mut TransferJob, file: &mut File) -> Result<()> {
        file.sync_all()?;
        job.bytes = file.metadata()?.len();
        job.status = Status::Paused;
        self.save(job)
    }
    async fn finish(&self, job: &mut TransferJob, file: File, options: &RunOptions) -> Result<()> {
        let was_publishing = job.status == Status::Publishing;
        file.sync_all()?;
        let limit = self.limit;
        let integrity = tokio::task::spawn_blocking(move || files::hash(file, limit))
            .await
            .map_err(|_| Error::State("transfer hash worker stopped"))??;
        files::owned_part(job)?;
        if job.resource.size.is_some_and(|n| n != integrity.bytes)
            || job
                .resource
                .md5
                .as_ref()
                .is_some_and(|m| *m != integrity.md5)
            || was_publishing && job.integrity.as_ref() != Some(&integrity)
        {
            return Err(Error::Media("final transfer integrity mismatch"));
        }
        if options.control.checkpoint()? == TransferDirective::Pause {
            if !was_publishing {
                job.status = Status::Paused;
                self.save(job)?;
            }
            return Ok(());
        }
        job.bytes = integrity.bytes;
        job.integrity = Some(integrity);
        job.status = Status::Publishing;
        self.save(job)?;
        // The journal is durable before creating the target. No restart or
        // refresh may truncate a part after this point: it could be linked.
        if !files::target_is_owned(job)? {
            files::absent(&job.target)?;
            fs::hard_link(&job.part, &job.target)?;
        }
        files::sync_parent(job)?;
        job.status = Status::Complete;
        job.cleanup_pending = true;
        if let Err(error) = self.save(job) {
            job.status = Status::Publishing;
            return Err(error);
        }
        self.cleanup(job);
        Ok(())
    }
    async fn verify_completed(&self, job: &mut TransferJob) -> Result<()> {
        if !files::target_is_owned(job)? {
            return Err(Error::State("completed transfer file changed identity"));
        }
        let file = File::open(&job.target)?;
        let limit = self.limit;
        let actual = tokio::task::spawn_blocking(move || files::hash(file, limit))
            .await
            .map_err(|_| Error::State("transfer hash worker stopped"))??;
        if job.integrity.as_ref() != Some(&actual) || !files::target_is_owned(job)? {
            return Err(Error::Media("completed transfer changed"));
        }
        self.cleanup(job);
        Ok(())
    }
    fn cleanup(&self, job: &mut TransferJob) {
        job.cleanup_pending = files::remove_part(job).is_err();
        // The completion was already committed. Cleanup failure does not
        // downgrade it or make a new network request on recovery.
        let _ = self.save(job);
    }
}
