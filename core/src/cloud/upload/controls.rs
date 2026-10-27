use super::*;
use manager::{CONTROL, JOBS};
impl CloudUploads {
    pub async fn control(&self, id: &str, action: UploadControl) -> Result<CloudView> {
        types::valid_id(id)?;
        self.session.with_token(&self.token, |_| {
            self.store.transaction(|tx| {
                let task: Task = serde_json::from_value(
                    tx.get(&self.owner, JOBS, id)?
                        .ok_or(Error::State("cloud upload not found for account"))?,
                )?;
                task.validate()?;
                if task.id != id {
                    return Err(Error::State("cloud upload identity mismatch"));
                }
                if matches!(task.phase, CloudPhase::Complete | CloudPhase::Cancelled) {
                    return Err(Error::State("cloud upload is terminal"));
                }
                if action == UploadControl::Cancel {
                    if task.uncertain() {
                        return Err(Error::MutationUncertain);
                    }
                    if task.phase == CloudPhase::Published {
                        return Err(Error::State("published cloud item cannot be cancelled"));
                    }
                }
                let old: Option<UploadControl> = tx
                    .get(&self.owner, CONTROL, id)?
                    .map(serde_json::from_value)
                    .transpose()?;
                if old == Some(UploadControl::Cancel) && action != UploadControl::Cancel {
                    return Err(Error::State("cancellation already requested"));
                }
                tx.put(&self.owner, CONTROL, id, &serde_json::to_value(action)?)
            })
        })?;
        self.stop_children(&self.read(id)?, action)?;
        match self.lock(id) {
            Ok(_lock) => {
                let mut task = self.read(id)?;
                self.halt(&mut task).await?;
                self.view(&task)
            }
            Err(Error::MutationBusy) => self.get(id),
            Err(error) => Err(error),
        }
    }
    fn stop_children(&self, task: &Task, action: UploadControl) -> Result<()> {
        let audio_id = task.child_id(None);
        if let Some(child) = self.oss.find(&audio_id)?
            && !matches!(child.status, OssStatus::Uploaded | OssStatus::Cancelled)
        {
            let oss_action = match action {
                UploadControl::Pause => OssControl::Pause,
                UploadControl::Cancel => OssControl::Cancel,
            };
            if let Err(error) = self.oss.control(&audio_id, oss_action)
                && !matches!(
                    self.oss.get(&audio_id)?.status,
                    OssStatus::Uploaded | OssStatus::Cancelled
                )
            {
                return Err(error);
            }
        }
        let action = match action {
            UploadControl::Pause => NosControl::Pause,
            UploadControl::Cancel => NosControl::Cancel,
        };
        for id in task.child_ids() {
            if let Some(child) = self.nos.find(&id)? {
                if matches!(child.status, NosStatus::Uploaded | NosStatus::Cancelled) {
                    continue;
                }
                if let Err(error) = self.nos.control(&id, action) {
                    // The child may have received its final acknowledgement
                    // between find and control; preserve that actual result.
                    if !matches!(
                        self.nos.get(&id)?.status,
                        NosStatus::Uploaded | NosStatus::Cancelled
                    ) {
                        return Err(error);
                    }
                }
            }
        }
        Ok(())
    }
    pub(super) async fn halt(&self, task: &mut Task) -> Result<bool> {
        if task.phase == CloudPhase::Complete {
            return Ok(false);
        }
        let Some(control) = self.requested(&task.id)? else {
            return Ok(false);
        };
        self.stop_children(task, control)?;
        if control == UploadControl::Cancel {
            if task.uncertain() {
                return Err(Error::MutationUncertain);
            }
            if task.phase == CloudPhase::Published {
                return Err(Error::State("published cloud item cannot be cancelled"));
            }
            if let Some(child) = self.oss.find(&task.child_id(None))?
                && !matches!(child.status, OssStatus::Uploaded | OssStatus::Cancelled)
            {
                // Persisted Cancel is only a request. Known multipart uploads
                // must acknowledge Abort; uncertain completed objects need
                // ownership/content readback before the parent becomes terminal.
                self.oss.cancel(&child.id).await?;
            }
            task.phase = CloudPhase::Cancelled;
            task.next_check_ms = 0;
            self.save(task)?;
        }
        Ok(true)
    }
    pub fn resume(&self, id: &str) -> Result<CloudView> {
        let _lock = self.lock(id)?;
        let task = self.read(id)?;
        if task.uncertain() {
            return Err(Error::MutationUncertain);
        }
        if matches!(task.phase, CloudPhase::Complete | CloudPhase::Cancelled)
            || self.requested(id)? == Some(UploadControl::Cancel)
        {
            return Err(Error::State("cloud task cannot resume"));
        }
        for child_id in task.child_ids() {
            if let Some(child) = self.nos.find(&child_id)?
                && child.requested == Some(NosControl::Pause)
            {
                self.nos.resume(&child_id)?;
            }
        }
        if let Some(child) = self.oss.find(&task.child_id(None))?
            && child.requested == Some(OssControl::Pause)
        {
            self.oss.resume(&child.id)?;
        }
        self.session.with_token(&self.token, |_| {
            self.store.transaction(|tx| {
                let control: Option<UploadControl> = tx
                    .get(&self.owner, CONTROL, id)?
                    .map(serde_json::from_value)
                    .transpose()?;
                if control == Some(UploadControl::Cancel) {
                    return Err(Error::State("cancellation already requested"));
                }
                tx.delete(&self.owner, CONTROL, id)
            })
        })?;
        self.view(&task)
    }
    pub async fn renew_token(&self, id: &str) -> Result<CloudView> {
        let _lock = self.lock(id)?;
        let task = self.read(id)?;
        let child = match task.phase {
            CloudPhase::AudioUploading
                if task.allocation.as_ref().and_then(|v| v["channel"].as_u64()) == Some(1) =>
            {
                task.child_id(None)
            }
            CloudPhase::AttachmentsUploading => task
                .attachments
                .iter()
                .find(|a| a.phase == AttachmentPhase::Uploading)
                .map(|a| task.child_id(Some(a.kind)))
                .ok_or(Error::State("no unfinished attachment transfer"))?,
            _ => return Err(Error::State("no current NOS transfer to renew")),
        };
        self.nos.renew_token(&child).await?;
        self.view(&task)
    }
}
