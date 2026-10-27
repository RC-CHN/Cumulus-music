use super::*;
use crate::cloud::source;
use serde_json::Value;
use std::sync::atomic::Ordering;
fn offset(value: &Value, size: u64) -> Result<u64> {
    value["offset"]
        .as_u64()
        .filter(|v| *v <= size)
        .ok_or(Error::Response("invalid acknowledged NOS offset"))
}
impl NosUploads {
    pub async fn run(&self, id: &str, options: NosRunOptions) -> Result<NosView> {
        if options.chunk_budget == Some(0) {
            return Err(Error::InvalidInput("NOS chunk budget must be positive"));
        }
        let _lock = self.lock(id)?;
        let mut job = self.read(id)?;
        if job.status == NosStatus::Uploaded {
            return self.view(&job);
        }
        if job.status == NosStatus::Cancelled {
            return Err(Error::State("NOS upload was cancelled"));
        }
        if self.apply_control(&mut job)? {
            return self.view(&job);
        }
        let result = self.advance(&mut job, &options).await;
        if let Err(error) = result {
            if matches!(error, Error::Cancelled)
                && matches!(job.status, NosStatus::Paused | NosStatus::Cancelled)
            {
                return self.view(&job);
            }
            if matches!(error, Error::StaleResponse | Error::Storage(_)) {
                return Err(error);
            }
            let ambiguous = matches!(error, Error::MutationUncertain)
                || job
                    .pending
                    .as_ref()
                    .is_some_and(|p| job.context.is_empty() || p.complete);
            job.status = if ambiguous {
                NosStatus::Ambiguous
            } else {
                NosStatus::Error
            };
            job.error = Some(
                if ambiguous {
                    "write_outcome_unknown"
                } else {
                    match error.root() {
                        Error::HttpStatus {
                            status: 401 | 403, ..
                        } => "object_authentication_failed",
                        Error::Media(_) => "source_changed",
                        Error::Transport(_) => "transport_failed",
                        _ => "upload_incomplete",
                    }
                }
                .into(),
            );
            self.save(&job)?;
            return Err(if ambiguous {
                Error::MutationUncertain
            } else {
                error
            });
        }
        self.view(&job)
    }
    async fn advance(&self, job: &mut Job, options: &NosRunOptions) -> Result<()> {
        let reader = job.source.open_async().await?;
        self.guard()?;
        if job
            .pending
            .as_ref()
            .is_some_and(|p| job.context.is_empty() || p.complete)
        {
            return Err(Error::MutationUncertain);
        }
        let mut renewed = false;
        if !job.context.is_empty() {
            match self.send(job, None, &mut renewed).await {
                Ok(response) => {
                    let remote = offset(&response, job.source.bytes)?;
                    let upper = job.pending.as_ref().map_or(job.offset, |p| p.end);
                    if remote > upper {
                        return Err(Error::Response("NOS offset exceeds this task's sent bytes"));
                    }
                    job.offset = remote;
                    job.pending = None;
                }
                Err(Error::HttpStatus { status: 404, .. }) => {
                    job.offset = 0;
                    job.context.clear();
                    job.pending = None;
                }
                Err(error) => return Err(error),
            }
            self.save(job)?;
        }
        job.status = NosStatus::Uploading;
        job.error = None;
        self.save(job)?;
        let mut chunks = 0;
        while job.offset < job.source.bytes {
            self.guard()?;
            if self.apply_control(job)? {
                return Ok(());
            }
            if options.stop.load(Ordering::Acquire)
                || options.chunk_budget.is_some_and(|n| chunks >= n)
            {
                job.status = NosStatus::Paused;
                self.save(job)?;
                return Ok(());
            }
            let length = (job.source.bytes - job.offset).min(job.chunk_size as u64) as usize;
            let bytes = source::chunk(reader.clone(), job.offset, length).await?;
            self.guard()?;
            // Honor a pause/cancel that arrived during file IO before sending.
            if self.apply_control(job)? {
                return Ok(());
            }
            let end = job.offset + bytes.len() as u64;
            let complete = end == job.source.bytes;
            if !self.prepare_chunk(
                job,
                Pending {
                    offset: job.offset,
                    end,
                    complete,
                },
            )? {
                return Ok(());
            }
            let response = self.send(job, Some(bytes), &mut renewed).await?;
            if offset(&response, job.source.bytes)? != end {
                return Err(Error::Response(
                    "NOS did not acknowledge the complete chunk",
                ));
            }
            let context = match response.get("context") {
                Some(Value::String(s)) => s.clone(),
                None => job.context.clone(),
                _ => return Err(Error::Response("invalid NOS context")),
            };
            if !complete {
                types::valid_text(&context, 16384)
                    .map_err(|_| Error::Response("NOS omitted resumable context"))?;
            }
            if complete {
                job.source.verify_async(reader.clone()).await?;
                self.guard()?;
            }
            job.offset = end;
            job.context = context;
            job.pending = None;
            if complete {
                job.status = NosStatus::Uploaded;
            }
            self.save(job)?;
            chunks += 1;
        }
        if job.status != NosStatus::Uploaded {
            return Err(Error::MutationUncertain);
        }
        Ok(())
    }
}
