use super::*;
impl OssUploads {
    pub(super) async fn reconcile_parts(&self, job: &mut Job) -> Result<()> {
        let upload_id = job
            .upload_id
            .clone()
            .ok_or(Error::State("missing OSS upload ID"))?;
        let mut marker = 0;
        let mut remote = std::collections::BTreeSet::new();
        loop {
            let page = self
                .call(
                    job,
                    OssOperation::Parts {
                        upload_id: upload_id.clone(),
                        marker,
                    },
                )
                .await?
                .parts(&job.allocation, &upload_id, marker)?;
            for part in page.parts {
                let local = job
                    .parts
                    .get(&part.number)
                    .ok_or(Error::MutationUncertain)?;
                if !remote.insert(part.number)
                    || part.bytes != local.bytes
                    || part.etag != local.md5
                {
                    return Err(Error::MutationUncertain);
                }
            }
            match page.next_marker {
                Some(next) => marker = next,
                None => break,
            }
        }
        for (number, part) in &mut job.parts {
            part.confirmed = remote.contains(number);
        }
        job.pending = None;
        self.save(job)
    }
    pub(super) async fn reconcile_complete(&self, job: &mut Job) -> Result<()> {
        self.call(job, OssOperation::Head)
            .await?
            .object_headers(&job.id, job.source.bytes)?;
        let completion = self
            .call(
                job,
                OssOperation::Verify {
                    bytes: job.source.bytes,
                },
            )
            .await?
            .verified_object(
                &job.id,
                job.source.bytes,
                &job.source.md5,
                &job.source.sha256,
            )?;
        job.completion = Some(completion);
        job.pending = None;
        job.status = OssStatus::Uploaded;
        job.error = None;
        self.save(job)
    }
    pub async fn reconcile(&self, id: &str) -> Result<OssView> {
        let _lock = self.lock(id)?;
        let mut job = self.read(id)?;
        if !matches!(job.pending, Some(Pending::Complete | Pending::Put)) {
            return Err(Error::State("OSS task is not awaiting object readback"));
        }
        self.reconcile_complete(&mut job).await?;
        self.view(&job)
    }
    pub async fn cancel(&self, id: &str) -> Result<OssView> {
        let current = self.get(id)?;
        if matches!(current.status, OssStatus::Uploaded | OssStatus::Cancelled) {
            return Ok(current);
        }
        if let Err(error) = self.control(id, OssControl::Cancel) {
            let current = self.get(id)?;
            if matches!(current.status, OssStatus::Uploaded | OssStatus::Cancelled) {
                return Ok(current);
            }
            return Err(error);
        }
        let _lock = self.lock(id)?;
        let mut job = self.read(id)?;
        self.cancel_locked(&mut job).await?;
        self.view(&job)
    }
    pub(super) async fn cancel_locked(&self, job: &mut Job) -> Result<()> {
        if job.status == OssStatus::Cancelled {
            return Ok(());
        }
        if matches!(job.pending, Some(Pending::Complete | Pending::Put)) {
            self.reconcile_complete(job).await?;
        }
        if job.status == OssStatus::Uploaded {
            return Ok(());
        } // Retain the accepted object; never DeleteObject.
        if let Some(upload_id) = job.upload_id.clone() {
            job.pending = Some(Pending::Abort);
            self.save(job)?;
            let result = self
                .call(job, OssOperation::Abort { upload_id })
                .await
                .and_then(|r| r.check(204));
            if let Err(error) = result
                && !matches!(&error,Error::HttpStatus{status:404,body:Some(body),..} if body["code"]=="NoSuchUpload")
            {
                return Err(error);
            }
        } else if job.pending.is_some() {
            return Err(Error::MutationUncertain);
        }
        job.status = OssStatus::Cancelled;
        job.pending = None;
        job.error = None;
        self.save(job)
    }
    pub(super) async fn halt(&self, job: &mut Job) -> Result<bool> {
        match self.requested(&job.id)? {
            Some(OssControl::Cancel) => {
                self.cancel_locked(job).await?;
                Ok(true)
            }
            Some(OssControl::Pause) => {
                job.status = OssStatus::Paused;
                self.save(job)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}
