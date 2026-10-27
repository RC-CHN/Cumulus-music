use super::*;

pub(super) enum AudioAllocation {
    Nos(NosAllocation),
    Oss(OssAllocation),
}
impl AudioAllocation {
    pub fn parse(value: &Value) -> Result<Self> {
        match value["channel"].as_u64() {
            Some(1) => NosAllocation::parse(value).map(Self::Nos),
            Some(3) => OssAllocation::parse(value).map(Self::Oss),
            _ => Err(Error::Response(
                "unsupported server-selected upload channel",
            )),
        }
    }
    pub fn resource_id(&self) -> &str {
        match self {
            Self::Nos(a) => a.resource_id(),
            Self::Oss(a) => a.resource_id(),
        }
    }
    pub fn object(&self) -> (&str, &str) {
        match self {
            Self::Nos(a) => (a.bucket(), a.object_key()),
            Self::Oss(a) => (a.bucket(), a.object_key()),
        }
    }
}
impl CloudUploads {
    pub(super) async fn prepare_audio(&self, task: &mut Task) -> Result<()> {
        let value = task
            .allocation
            .as_ref()
            .ok_or(Error::State("missing audio allocation"))?;
        if !matches!(value["channel"].as_u64(), Some(1 | 3)) {
            task.error = Some(UploadIssue::new("unsupported_upload_channel"));
            self.save(task)?;
        }
        let allocation = AudioAllocation::parse(value)?;
        let id = task.child_id(None);
        match allocation {
            AudioAllocation::Nos(allocation) => {
                if self.nos.find(&id)?.is_none() {
                    let node = self.nos.discover(allocation.bucket()).await?;
                    if self.halt(task).await? {
                        return Ok(());
                    }
                    self.nos
                        .create(
                            Some(&id),
                            &task.source.path,
                            allocation,
                            node,
                            &task.source.md5,
                            "application/octet-stream",
                        )
                        .await?;
                }
            }
            AudioAllocation::Oss(allocation) => {
                if self.oss.find(&id)?.is_none() {
                    self.oss
                        .create(Some(&id), &task.source.path, allocation, &task.source.md5)
                        .await?;
                }
            }
        }
        task.phase = CloudPhase::AudioUploading;
        task.error = None;
        self.save(task)
    }
    pub(super) async fn transfer_audio(
        &self,
        task: &Task,
        options: &CloudRunOptions,
    ) -> Result<bool> {
        let id = task.child_id(None);
        match task.allocation.as_ref().and_then(|v| v["channel"].as_u64()) {
            Some(1) => Ok(self
                .nos
                .run(
                    &id,
                    NosRunOptions {
                        chunk_budget: options.chunk_budget,
                        ..Default::default()
                    },
                )
                .await?
                .status
                == NosStatus::Uploaded),
            Some(3) => Ok(self
                .oss
                .run(
                    &id,
                    OssRunOptions {
                        chunk_budget: options.chunk_budget,
                        ..Default::default()
                    },
                )
                .await?
                .status
                == OssStatus::Uploaded),
            _ => Err(Error::State("invalid active cloud transfer channel")),
        }
    }
    /// Native-only scoped credentials. No recovered automatic STS renewal API
    /// exists; this must never allocate a replacement object or use NOS renewal.
    pub fn replace_oss_credentials(
        &self,
        id: &str,
        allocation: OssAllocation,
    ) -> Result<CloudView> {
        let _lock = self.lock(id)?;
        let task = self.read(id)?;
        self.oss_task(&task)?;
        self.oss
            .replace_credentials(&task.child_id(None), allocation)?;
        self.view(&task)
    }
    /// Read-only ownership, length and content checks after an unknown PUT or
    /// CompleteMultipartUpload outcome. Does not publish or replay an upload.
    pub async fn reconcile_transfer(&self, id: &str) -> Result<CloudView> {
        let _lock = self.lock(id)?;
        let mut task = self.read(id)?;
        self.oss_task(&task)?;
        let child = task.child_id(None);
        if self.oss.get(&child)?.status != OssStatus::Uploaded {
            self.oss.reconcile(&child).await?;
        }
        if !self.halt(&mut task).await? {
            task.phase = CloudPhase::AudioUploaded;
            task.error = None;
            self.save(&task)?;
        }
        self.view(&task)
    }
    fn oss_task(&self, task: &Task) -> Result<()> {
        if !matches!(
            task.phase,
            CloudPhase::Allocated | CloudPhase::AudioUploading
        ) || task.allocation.as_ref().and_then(|v| v["channel"].as_u64()) != Some(3)
        {
            return Err(Error::State("no unfinished OSS audio transfer"));
        }
        Ok(())
    }
}
