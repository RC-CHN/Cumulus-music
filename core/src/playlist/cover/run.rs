use super::{types::Task, *};
use crate::{
    account::SessionToken,
    cloud::{
        CloudApi,
        nos::{NosAllocation, NosControl, NosRunOptions, NosStatus},
    },
    playlist::{PlaylistCommand, PlaylistMutation, PlaylistPhase},
};

impl PlaylistCovers {
    /// Explicitly advance the object transfer. Never binds the playlist, and
    /// never re-allocates after an unknown allocation or final NOS response.
    pub async fn upload(
        &self,
        token: &SessionToken,
        id: &str,
        options: NosRunOptions,
    ) -> Result<CoverTaskView> {
        let _lock = self.lock(token, id)?;
        let mut task = self.read(token, id)?;
        if task.phase == CoverPhase::Uploaded {
            return self.view(&task);
        }
        match task.phase {
            CoverPhase::Prepared | CoverPhase::Uploading => (),
            CoverPhase::Allocating => return Err(Error::MutationUncertain),
            _ => return Err(Error::State("cover task is not ready to upload")),
        }
        self.check_target(token, &task.playlist_id, &task.revision)
            .await?;
        if options.stop.load(std::sync::atomic::Ordering::Acquire) {
            return self.view(&task);
        }
        if task.phase == CoverPhase::Prepared {
            task.phase = CoverPhase::Allocating;
            task.may_leave_object = true;
            self.save(token, &task)?;
            let result = self
                .session
                .run_for(token, |api, _| async move {
                    CloudApi::new(api)
                        .allocate_playlist_cover("cover.jpg")
                        .await
                })
                .await;
            let allocation = match result {
                Ok(value) => NosAllocation::parse(&value)?,
                Err(error) => {
                    if matches!(error.root(), Error::Service { code, .. }
                        if code.as_i64().is_some_and(|c| [400,403,405,406,413,415].contains(&c)))
                    {
                        task.phase = CoverPhase::AllocationRejected;
                        task.may_leave_object = false;
                        task.error = Some("allocation_rejected".into());
                        self.save(token, &task)?;
                    }
                    return Err(error);
                }
            };
            task.allocation = Some(allocation);
            task.phase = CoverPhase::Uploading;
            task.error = None;
            self.save(token, &task)?;
        }
        let allocation = task
            .allocation
            .clone()
            .ok_or(Error::State("missing cover allocation"))?;
        if self.nos.find(id)?.is_none() {
            let path = self.materialize(&task)?;
            self.guard(token)?;
            let node = self.nos.discover(allocation.bucket()).await?;
            self.nos
                .create(Some(id), &path, allocation, node, &task.md5, "image/jpeg")
                .await?;
        }
        let child = self.nos.get(id)?;
        if matches!(child.status, NosStatus::Paused | NosStatus::Error) {
            self.nos.resume(id)?;
        }
        let child = self.nos.run(id, options).await?;
        self.guard(token)?;
        if child.status == NosStatus::Uploaded {
            task.phase = CoverPhase::Uploaded;
            task.error = None;
            self.save(token, &task)?;
        }
        self.view(&task)
    }
    pub fn retry_allocation(&self, token: &SessionToken, id: &str) -> Result<CoverTaskView> {
        let _lock = self.lock(token, id)?;
        let mut task = self.read(token, id)?;
        if task.phase != CoverPhase::AllocationRejected {
            return Err(Error::State("only a rejected allocation can be retried"));
        }
        task.phase = CoverPhase::Prepared;
        task.error = None;
        self.save(token, &task)?;
        self.view(&task)
    }
    pub fn pause(&self, token: &SessionToken, id: &str) -> Result<CoverTaskView> {
        let task = self.read(token, id)?;
        if task.phase != CoverPhase::Uploading {
            return Err(Error::State("cover transfer has not started"));
        }
        self.nos.control(id, NosControl::Pause)?;
        self.view(&task)
    }
    pub async fn renew(&self, token: &SessionToken, id: &str) -> Result<CoverTaskView> {
        let _lock = self.lock(token, id)?;
        let task = self.read(token, id)?;
        if task.phase != CoverPhase::Uploading {
            return Err(Error::State("cover transfer does not need renewal"));
        }
        self.nos.renew_token(id).await?;
        self.view(&task)
    }
    /// Explicit acknowledgement of a refreshed editor. Never automatically
    /// overwrite a playlist which changed while its image was uploading.
    pub async fn rebase(
        &self,
        token: &SessionToken,
        id: &str,
        revision: &str,
    ) -> Result<CoverTaskView> {
        let _lock = self.lock(token, id)?;
        let mut task = self.read(token, id)?;
        if !matches!(
            task.phase,
            CoverPhase::Prepared | CoverPhase::Uploading | CoverPhase::Uploaded
        ) || self
            .playlists
            .operations(token)?
            .iter()
            .any(|r| r.operation_id == task.binding_id)
        {
            return Err(Error::State("cover binding has already started"));
        }
        self.check_target(token, &task.playlist_id, revision)
            .await?;
        task.revision = revision.into();
        task.error = None;
        self.save(token, &task)?;
        self.view(&task)
    }
    pub async fn bind(&self, token: &SessionToken, id: &str) -> Result<CoverTaskView> {
        let _lock = self.lock(token, id)?;
        let mut task = self.read(token, id)?;
        if task.phase.terminal() {
            return self.view(&task);
        }
        if !matches!(task.phase, CoverPhase::Uploaded | CoverPhase::Binding) {
            return Err(Error::State("cover upload is not confirmed"));
        }
        let allocation = task
            .allocation
            .as_ref()
            .ok_or(Error::State("cover document missing"))?;
        self.nos.confirm_object(id, allocation, &task.md5)?;
        let document_id = allocation.resource_id().to_owned();
        task.phase = CoverPhase::Binding;
        self.save(token, &task)?;
        let result = self
            .playlists
            .execute_identified(
                token,
                PlaylistCommand::Cover {
                    id: task.playlist_id.clone(),
                    revision: task.revision.clone(),
                    document_id,
                },
                &task.binding_id,
            )
            .await;
        match result {
            Ok(record) => self.accept_binding(token, &mut task, record)?,
            Err(error) => {
                // Absence of the child log proves no binding request started.
                // A late account reply must retain responsibility under its
                // original owner; do not mark it successful for the new one.
                if self.guard(token).is_ok()
                    && !self
                        .playlists
                        .operations(token)?
                        .iter()
                        .any(|r| r.operation_id == task.binding_id)
                {
                    task.phase = CoverPhase::Uploaded;
                    task.error = Some(
                        if matches!(error, Error::QueueChanged) {
                            "playlist_changed"
                        } else {
                            "binding_not_started"
                        }
                        .into(),
                    );
                    self.save(token, &task)?;
                }
                return Err(error);
            }
        }
        self.view(&task)
    }
    fn accept_binding(
        &self,
        token: &SessionToken,
        task: &mut Task,
        record: PlaylistMutation,
    ) -> Result<()> {
        task.binding = Some(record.phase);
        task.phase = match record.phase {
            PlaylistPhase::Confirmed | PlaylistPhase::Unchanged => CoverPhase::Confirmed,
            PlaylistPhase::Rejected => CoverPhase::Rejected,
            _ => CoverPhase::Binding,
        };
        task.error = record.error_class;
        if task.phase == CoverPhase::Confirmed {
            task.may_leave_object = false;
        }
        self.save(token, task)
    }
    /// Reads the existing child and the official playlist detail; no mutation
    /// is issued. Before-child crashes return to Uploaded, requiring a click.
    pub async fn reconcile(&self, token: &SessionToken, id: &str) -> Result<CoverTaskView> {
        let _lock = self.lock(token, id)?;
        let mut task = self.read(token, id)?;
        if task.phase != CoverPhase::Binding {
            return self.view(&task);
        }
        if self
            .playlists
            .operations(token)?
            .iter()
            .any(|r| r.operation_id == task.binding_id)
        {
            let record = self.playlists.reconcile(token, &task.binding_id).await?;
            self.accept_binding(token, &mut task, record)?;
        } else {
            task.phase = CoverPhase::Uploaded;
            task.error = None;
            self.save(token, &task)?;
        }
        self.view(&task)
    }
    pub fn abandon(&self, token: &SessionToken, id: &str) -> Result<CoverTaskView> {
        let _lock = self.lock(token, id)?;
        let mut task = self.read(token, id)?;
        if task.phase.terminal() {
            return self.view(&task);
        }
        if task.phase == CoverPhase::Binding {
            return Err(Error::MutationUncertain);
        }
        if let Some(child) = self.nos.find(id)?
            && !matches!(child.status, NosStatus::Uploaded | NosStatus::Cancelled)
        {
            self.nos.control(id, NosControl::Cancel)?;
        }
        task.phase = CoverPhase::Abandoned;
        self.save(token, &task)?;
        self.view(&task)
    }
}
