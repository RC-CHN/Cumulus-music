use super::*;
impl CloudUploads {
    pub async fn run(&self, id: &str, options: CloudRunOptions) -> Result<CloudView> {
        if options.chunk_budget == Some(0) {
            return Err(Error::InvalidInput("cloud chunk budget must be positive"));
        }
        let _lock = self.lock(id)?;
        let mut task = self.read(id)?;
        if task.phase == CloudPhase::Complete {
            return self.view(&task);
        }
        if task.phase == CloudPhase::Cancelled {
            return Err(Error::State("cloud upload was cancelled"));
        }
        if task.uncertain() {
            return Err(Error::MutationUncertain);
        }
        match self.advance(&mut task, &options).await {
            Ok(()) => self.view(&task),
            Err(Error::Cancelled) if self.requested(id)?.is_some() => {
                // begin may have changed an in-memory phase before a failed
                // transaction; only the committed state is authoritative.
                let mut saved = self.read(id)?;
                self.halt(&mut saved).await?;
                self.view(&saved)
            }
            Err(error) => Err(error),
        }
    }
    async fn advance(&self, task: &mut Task, options: &CloudRunOptions) -> Result<()> {
        if self.halt(task).await? {
            return Ok(());
        }
        if task.phase == CloudPhase::Failed {
            if !options.retry_rejected {
                return Err(Error::State(
                    "cloud write rejection requires explicit retry",
                ));
            }
            task.phase = task
                .retry_phase
                .take()
                .ok_or(Error::State("cloud failure is not retryable"))?;
            task.error = None;
            self.save(task)?;
        }
        if matches!(
            task.phase,
            CloudPhase::Queued
                | CloudPhase::Checked
                | CloudPhase::Allocated
                | CloudPhase::AudioUploading
                | CloudPhase::AudioUploaded
                | CloudPhase::AttachmentsUploading
        ) {
            self.verify_inputs(task).await?;
        }
        loop {
            self.guard()?;
            if self.halt(task).await? {
                return Ok(());
            }
            match task.phase {
                CloudPhase::Queued => {
                    let md5 = task.source.md5.clone();
                    let size = task.source.bytes;
                    let name = types::file_name(&task.source)?.to_owned();
                    let result = self
                        .call(|api| async move { api.check(&md5, size, &name, false).await })
                        .await?;
                    let ticket = match &result["songId"] {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        _ => return Err(Error::Response("invalid upload check ticket")),
                    };
                    task.decision = Some(Decision {
                        need_upload: result["needUpload"]
                            .as_bool()
                            .ok_or(Error::Response("missing upload decision"))?,
                        ticket,
                    });
                    task.phase = CloudPhase::Checked;
                    self.save(task)?;
                }
                CloudPhase::Checked => {
                    if task
                        .decision
                        .as_ref()
                        .ok_or(Error::State("missing upload check"))?
                        .need_upload
                    {
                        let name = types::file_name(&task.source)?.to_owned();
                        let md5 = task.source.md5.clone();
                        self.begin(task, CloudPhase::AllocationPending)?;
                        let result = self
                            .call(|api| async move { api.allocate(&name, &md5).await })
                            .await;
                        let allocation = self.rejected(task, CloudPhase::Checked, result)?;
                        task.allocation = Some(allocation);
                        task.phase = CloudPhase::Allocated;
                        task.error = None;
                    } else {
                        task.phase = CloudPhase::AudioUploaded;
                    }
                    self.save(task)?;
                }
                CloudPhase::Allocated => {
                    self.prepare_audio(task).await?;
                }
                CloudPhase::AudioUploading => {
                    let uploaded = self.transfer_audio(task, options).await?;
                    if self.halt(task).await? {
                        return Ok(());
                    }
                    if !uploaded {
                        return Ok(());
                    }
                    task.phase = CloudPhase::AudioUploaded;
                    self.save(task)?;
                }
                CloudPhase::AudioUploaded => {
                    task.phase = CloudPhase::AttachmentsUploading;
                    self.save(task)?;
                }
                CloudPhase::AttachmentsUploading => {
                    if !self.attachments(task, options).await? {
                        return Ok(());
                    }
                    if self.halt(task).await? {
                        return Ok(());
                    }
                    self.metadata(task).await?;
                }
                CloudPhase::Encoding => {
                    if !self.encoding(task).await? {
                        return Ok(());
                    }
                }
                CloudPhase::ReadyToPublish => self.publish(task).await?,
                CloudPhase::Published => {
                    if self.clock.now_ms()? >= task.next_check_ms {
                        self.readback(task).await?;
                    }
                    return Ok(());
                }
                CloudPhase::Complete | CloudPhase::Cancelled | CloudPhase::Failed => return Ok(()),
                _ => return Err(Error::MutationUncertain),
            }
        }
    }
}
