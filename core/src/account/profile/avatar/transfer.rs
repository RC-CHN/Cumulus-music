use super::{
    AvatarEditor, AvatarIssue, AvatarPhase, AvatarTaskView, SelfProfileView, Task, safe_avatar,
};
use crate::{
    Error, Result,
    account::SessionToken,
    cloud::{
        CloudApi,
        nos::{NosAllocation, NosRunOptions, NosStatus},
    },
};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

impl AvatarEditor {
    pub async fn upload(
        &self,
        token: &SessionToken,
        id: &str,
        options: NosRunOptions,
    ) -> Result<AvatarTaskView> {
        let _lock = self.lock(token)?;
        let mut task = self.read(token, id)?;
        match task.phase {
            AvatarPhase::Prepared | AvatarPhase::Uploading => (),
            AvatarPhase::Allocating | AvatarPhase::Uploaded => return self.view(token, &task),
            _ => return Err(Error::State("avatar task is not ready to upload")),
        }
        if options.stop.load(Ordering::Acquire) {
            return self.view(token, &task);
        }
        if task.phase == AvatarPhase::Prepared {
            Self::encoded(&task)?;
            let avatar = self
                .detail(token)
                .await?
                .avatar_url
                .ok_or(Error::Response("current avatar cannot be verified"))?;
            if options.stop.load(Ordering::Acquire) {
                return self.view(token, &task);
            }
            task.baseline = Some(avatar);
            task.phase = AvatarPhase::Allocating;
            task.may_leave_object = true;
            self.save(token, &task)?;
            let allocation = self
                .session
                .run_for(token, |api, _| async move {
                    CloudApi::new(api).allocate_avatar().await
                })
                .await;
            let value = match allocation {
                Ok(value) => value,
                Err(error) if matches!(error.root(), Error::RequestNotSent(_)) => {
                    task.phase = AvatarPhase::Prepared;
                    task.may_leave_object = false;
                    self.save(token, &task)?;
                    return Err(error);
                }
                Err(_) => return self.view(token, &task),
            };
            let allocation = NosAllocation::parse(&value)?;
            super::model::object_url(&allocation)?;
            task.imgid = Some(value["docId"].clone());
            task.allocation = Some(allocation);
            task.phase = AvatarPhase::Uploading;
            self.save(token, &task)?;
        }
        if options.stop.load(Ordering::Acquire) {
            return self.view(token, &task);
        }
        let allocation = task
            .allocation
            .as_ref()
            .ok_or(Error::State("avatar allocation missing"))?;
        if self.nos.find(id)?.is_none() {
            let path = self.materialize(&task)?;
            self.guard(token)?;
            let node = self.nos.discover(allocation.bucket()).await?;
            self.nos
                .create(
                    Some(id),
                    &path,
                    allocation.clone(),
                    node,
                    &task.md5,
                    "image/jpeg",
                )
                .await?;
        }
        let transfer = self.nos.get(id)?;
        if transfer.status == NosStatus::Ambiguous {
            return self.view(token, &task);
        }
        if matches!(transfer.status, NosStatus::Paused | NosStatus::Error) {
            self.nos.resume(id)?;
        }
        let result = self.nos.run(id, options).await;
        self.guard(token)?;
        match result {
            Ok(transfer) if transfer.status == NosStatus::Uploaded => {
                self.nos.confirm_object(id, allocation, &task.md5)?;
                task.phase = AvatarPhase::Uploaded;
                self.save(token, &task)?;
            }
            Ok(_) => (),
            Err(error) => {
                if !matches!(
                    self.nos.get(id)?.status,
                    NosStatus::Error | NosStatus::Ambiguous
                ) {
                    return Err(error);
                }
            }
        }
        self.view(token, &task)
    }

    pub async fn bind(&self, token: &SessionToken, id: &str) -> Result<AvatarTaskView> {
        let _lock = self.lock(token)?;
        let mut task = self.read(token, id)?;
        if task.phase == AvatarPhase::Binding || task.phase.terminal() {
            // Returning the journal is idempotent, including after lost IPC.
            return self.view(token, &task);
        }
        if task.phase != AvatarPhase::Uploaded {
            return Err(Error::State("avatar upload is not confirmed"));
        }
        let allocation = task
            .allocation
            .as_ref()
            .ok_or(Error::State("avatar allocation missing"))?;
        self.nos.confirm_object(id, allocation, &task.md5)?;
        let observed = self
            .detail(token)
            .await?
            .avatar_url
            .ok_or(Error::Response("current avatar cannot be verified"))?;
        if task.baseline.as_deref() != Some(observed.as_str()) {
            task.error = Some(AvatarIssue::Changed);
            self.save(token, &task)?;
            return self.view(token, &task);
        }
        let url = super::model::object_url(allocation)?;
        let imgid = task
            .imgid
            .as_ref()
            .ok_or(Error::State("missing avatar document ID"))?;
        let payload = json!({"imgid":imgid,"url":url.as_str()});
        task.phase = AvatarPhase::Binding;
        task.error = Some(AvatarIssue::BindUncertain);
        self.save(token, &task)?;
        // M15:3648, M2167:324–346. Preserve the allocated docId's type.
        let reply = self
            .session
            .run_for(token, |api, _| async move {
                api.call("/api/user/avatar/upload/v1", payload, false, &[200])
                    .await
            })
            .await;
        match reply {
            Ok(reply) => {
                task.acknowledged = safe_avatar(&reply["url"]);
                self.save(token, &task)?;
                match self.observe(token, task, None).await {
                    Ok(view) => Ok(view),
                    Err(_) => self.get(token, id),
                }
            }
            Err(error) => {
                let rejected = if error.is_service_code(405) {
                    Some(AvatarIssue::RateLimited)
                } else if error.is_service_code(533) {
                    Some(AvatarIssue::PhoneRequired)
                } else if matches!(error.root(), Error::RequestNotSent(_)) {
                    Some(AvatarIssue::BindNotSent)
                } else {
                    None
                };
                if let Some(issue) = rejected {
                    task.phase = AvatarPhase::Rejected;
                    task.error = Some(issue);
                    self.save(token, &task)?;
                }
                self.view(token, &task)
            }
        }
    }

    pub async fn reconcile(&self, token: &SessionToken, id: &str) -> Result<AvatarTaskView> {
        let _lock = self.lock(token)?;
        let task = self.read(token, id)?;
        if task.phase != AvatarPhase::Binding {
            return self.view(token, &task);
        }
        self.observe(token, task, None).await
    }

    pub async fn accept_current(
        &self,
        token: &SessionToken,
        id: &str,
        revision: &str,
    ) -> Result<AvatarTaskView> {
        let _lock = self.lock(token)?;
        let task = self.read(token, id)?;
        if task.phase.terminal() {
            return self.view(token, &task);
        }
        if task.phase != AvatarPhase::Binding
            || task.observed_revision().as_deref() != Some(revision)
        {
            return Err(Error::State("reconcile avatar before acceptance"));
        }
        self.observe(token, task, Some(revision)).await
    }

    async fn observe(
        &self,
        token: &SessionToken,
        mut task: Task,
        accept: Option<&str>,
    ) -> Result<AvatarTaskView> {
        let api = self.session.native_api(token)?;
        let profile = self.detail(token).await?;
        let Some(avatar) = profile.avatar_url else {
            task.observed = None;
            self.save(token, &task)?;
            if accept.is_some() {
                return Err(Error::State("current avatar cannot be verified"));
            }
            return self.view(token, &task);
        };
        let allocation = task
            .allocation
            .as_ref()
            .ok_or(Error::State("missing avatar allocation"))?;
        let requested = super::model::object_url(allocation)?;
        let requested = safe_avatar(&Value::String(requested.into()));
        // A different URL is not proof of this mutation. Do not infer that
        // avatarImgId equals docId, or equate paths on different CDN hosts.
        let confirmed = Some(&avatar) == requested.as_ref()
            || (Some(&avatar) == task.acknowledged.as_ref()
                && Some(&avatar) != task.baseline.as_ref());
        task.observed = Some(avatar.clone());
        let accepted = accept.is_some() && accept == task.observed_revision().as_deref();
        if confirmed || accepted {
            task.phase = if confirmed {
                AvatarPhase::Confirmed
            } else {
                AvatarPhase::AcceptedCurrentState
            };
            task.error = None;
            task.may_leave_object = !confirmed;
            task.validate()?;
            let verified = SelfProfileView {
                uid: profile.uid,
                avatar_url: Some(avatar),
                nickname: None,
                signature: None,
                gender: None,
                birthday: None,
                province: None,
                city: None,
                level: None,
            };
            self.session
                .commit_profile_edit(token, &api, &verified, |tx, uid| {
                    tx.put(uid, super::TASKS, &task.id, &serde_json::to_value(&task)?)
                })?;
        } else {
            self.save(token, &task)?;
            if accept.is_some() {
                return Err(Error::State("account avatar changed"));
            }
        }
        self.view(token, &task)
    }
}
