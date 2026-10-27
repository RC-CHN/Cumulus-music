use super::*;
use crate::cloud::source::{self, Reader};
use sha2::Digest;
use std::sync::atomic::Ordering;
use tokio::task::JoinSet;
impl OssUploads {
    pub async fn run(&self, id: &str, options: OssRunOptions) -> Result<OssView> {
        if options.chunk_budget == Some(0) {
            return Err(Error::InvalidInput("OSS chunk budget must be positive"));
        }
        let _lock = self.lock(id)?;
        let mut job = self.read(id)?;
        if job.status == OssStatus::Uploaded {
            return self.view(&job);
        }
        if job.status == OssStatus::Cancelled {
            return Err(Error::State("OSS task was cancelled"));
        }
        let result = self.advance(&mut job, &options).await;
        if let Err(error) = result {
            if matches!(error, Error::StaleResponse | Error::Storage(_)) {
                return Err(error);
            }
            if matches!(error, Error::Cancelled) && self.requested(id)?.is_some() {
                let mut saved = self.read(id)?;
                self.halt(&mut saved).await?;
                return self.view(&saved);
            }
            let ambiguous = matches!(error, Error::MutationUncertain)
                || matches!(
                    job.pending,
                    Some(Pending::Initiate | Pending::Put | Pending::Complete | Pending::Abort)
                );
            job.status = if ambiguous {
                OssStatus::Ambiguous
            } else {
                OssStatus::Error
            };
            job.error = Some(
                if ambiguous {
                    "write_outcome_unknown"
                } else {
                    match error.root() {
                        Error::Media(_) => "source_or_checksum_mismatch",
                        Error::HttpStatus {
                            status: 401 | 403, ..
                        } => "object_credentials_rejected",
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
    async fn advance(&self, job: &mut Job, options: &OssRunOptions) -> Result<()> {
        if self.halt(job).await? {
            return Ok(());
        }
        match job.pending {
            Some(Pending::Initiate | Pending::Abort) => return Err(Error::MutationUncertain),
            Some(Pending::Put | Pending::Complete) => return self.reconcile_complete(job).await,
            _ => {}
        }
        if options.stop.load(Ordering::Acquire) {
            job.status = OssStatus::Paused;
            return self.save(job);
        }
        let reader = job.source.open_async().await?;
        self.guard()?;
        if job.source.bytes <= job.part_size as u64 {
            let data = source::chunk(reader.clone(), 0, job.source.bytes as usize).await?;
            if hex::encode(md5::Md5::digest(&data)) != job.source.md5
                || hex::encode(sha2::Sha256::digest(&data)) != job.source.sha256
            {
                return Err(Error::Media("OSS source changed before PUT"));
            }
            job.status = OssStatus::Uploading;
            self.begin(job, Pending::Put)?;
            let result = self
                .call(job, OssOperation::Put(data))
                .await
                .and_then(|r| r.put_completed(&job.source.md5, job.crc64));
            self.reject(job, &result, OssPending::Put)?;
            let completion = result?;
            job.source.verify_async(reader).await?;
            self.guard()?;
            job.completion = Some(completion);
            job.status = OssStatus::Uploaded;
            job.pending = None;
            job.error = None;
            return self.save(job);
        }
        if job.upload_id.is_some() {
            self.reconcile_parts(job).await?;
        } else {
            job.status = OssStatus::Initiating;
            self.begin(job, Pending::Initiate)?;
            let result = self
                .call(job, OssOperation::Initiate)
                .await
                .and_then(|r| r.initiated(&job.allocation));
            self.reject(job, &result, OssPending::Initiate)?;
            job.upload_id = Some(result?);
            job.pending = None;
            self.save(job)?;
        }
        job.status = OssStatus::Uploading;
        job.error = None;
        self.save(job)?;
        if !self.parts(job, reader.clone(), options).await? {
            return Ok(());
        }
        job.source.verify_async(reader).await?;
        self.guard()?;
        let parts = job
            .parts
            .values()
            .map(|part| {
                if !part.confirmed {
                    return Err(Error::State("OSS completion contains unconfirmed part"));
                }
                Ok(PartTag {
                    number: part.number,
                    etag: part.md5.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if parts.len() != job.count() as usize {
            return Err(Error::State("OSS completion is missing parts"));
        }
        let upload_id = job
            .upload_id
            .clone()
            .ok_or(Error::State("missing OSS upload ID"))?;
        job.status = OssStatus::Completing;
        self.begin(job, Pending::Complete)?;
        let result = self
            .call(job, OssOperation::Complete { upload_id, parts })
            .await
            .and_then(|r| r.multipart_completed(&job.allocation, job.crc64));
        self.reject(job, &result, OssPending::Complete)?;
        job.completion = Some(result?);
        job.status = OssStatus::Uploaded;
        job.pending = None;
        job.error = None;
        self.save(job)
    }
    fn reject<T>(&self, job: &mut Job, result: &Result<T>, kind: OssPending) -> Result<()> {
        if let Err(Error::HttpStatus { status, .. }) = result {
            let excluded = match kind {
                OssPending::Initiate => [0, 408, 0],
                OssPending::Put => [0, 408, 409],
                _ => [404, 408, 409],
            };
            if (400..500).contains(status) && !excluded.contains(status) {
                job.pending = None;
                self.save(job)?;
            }
        }
        Ok(())
    }
    async fn parts(&self, job: &mut Job, reader: Reader, options: &OssRunOptions) -> Result<bool> {
        let mut sent = 0;
        loop {
            if self.halt(job).await? {
                return Ok(false);
            }
            let missing = (1..=job.count())
                .filter(|n| job.parts.get(n).is_none_or(|p| !p.confirmed))
                .collect::<Vec<_>>();
            if missing.is_empty() {
                return Ok(true);
            }
            let budget = options.chunk_budget.map_or(job.concurrency, |limit| {
                limit.saturating_sub(sent).min(job.concurrency)
            });
            if budget == 0 || options.stop.load(Ordering::Acquire) {
                job.status = OssStatus::Paused;
                self.save(job)?;
                return Ok(false);
            }
            let mut batch = Vec::new();
            for number in missing.into_iter().take(budget) {
                let bytes = job.part_bytes(number);
                let offset = (number - 1) as u64 * job.part_size as u64;
                let data = source::chunk(reader.clone(), offset, bytes as usize).await?;
                self.guard()?;
                let md5 = hex::encode(md5::Md5::digest(&data));
                if job.parts.get(&number).is_some_and(|p| p.md5 != md5) {
                    return Err(Error::Media("OSS source differs from recorded part"));
                }
                job.parts.insert(
                    number,
                    Part {
                        number,
                        bytes,
                        md5,
                        confirmed: false,
                    },
                );
                batch.push((number, data));
            }
            self.begin(
                job,
                Pending::Parts {
                    numbers: batch.iter().map(|(n, _)| *n).collect(),
                },
            )?;
            let mut workers = JoinSet::new();
            let mut error = None;
            for (number, data) in batch {
                let manager = self.clone();
                let snapshot = job.clone();
                let upload_id = job
                    .upload_id
                    .clone()
                    .ok_or(Error::State("missing OSS multipart ID"))?;
                workers.spawn(async move {
                    (
                        number,
                        manager
                            .call(
                                &snapshot,
                                OssOperation::Part {
                                    upload_id,
                                    number,
                                    data,
                                },
                            )
                            .await
                            .and_then(|r| r.part_etag()),
                    )
                });
                sent += 1;
            }
            // Drain every dispatched request even after a failure; retain all
            // accepted acknowledgements before deciding whether to continue.
            while let Some(result) = workers.join_next().await {
                let accepted: Result<()> = match result {
                    Ok((number, Ok(etag))) => {
                        let record = job
                            .parts
                            .get_mut(&number)
                            .ok_or(Error::State("missing sent OSS part"))?;
                        if etag != record.md5 {
                            Err(Error::Media("OSS part ETag mismatch"))
                        } else {
                            record.confirmed = true;
                            if let Some(Pending::Parts { numbers }) = &mut job.pending {
                                numbers.retain(|n| *n != number);
                            }
                            self.save(job)
                        }
                    }
                    Ok((_, Err(e))) => Err(e),
                    Err(_) => Err(Error::State("OSS part worker stopped")),
                };
                if let Err(e) = accepted {
                    // Storage/generation errors must win: run must not replace
                    // the committed pending journal with an in-memory guess.
                    if error.is_none() || matches!(e, Error::Storage(_) | Error::StaleResponse) {
                        error = Some(e);
                    }
                }
            }
            if let Some(error) = error {
                return Err(error);
            }
            job.pending = None;
            self.save(job)?;
        }
    }
}
