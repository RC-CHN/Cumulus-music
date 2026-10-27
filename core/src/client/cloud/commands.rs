use super::*;
impl MusicClient {
    pub async fn cloud_command(&self, epoch: &str, command: CloudCommand) -> Result<CloudResult> {
        let context = self.cloud.inner.context(epoch).await?;
        if matches!(
            command,
            CloudCommand::Upload { .. }
                | CloudCommand::Resume { .. }
                | CloudCommand::ResumeAll { .. }
        ) && let Some(snapshot) = context.library.cached()?
            && let (Some(size), Some(max)) = (snapshot.size, snapshot.max_size)
            && max > 0
            && size >= max
        {
            return Err(Error::State("cloud capacity exhausted"));
        }
        let command = match command {
            CloudCommand::PauseAll => CloudCommand::Pause {
                ids: context
                    .uploads
                    .list()?
                    .into_iter()
                    .filter(|v| {
                        !matches!(v.phase, CloudPhase::Complete | CloudPhase::Cancelled)
                            && v.requested.is_none()
                    })
                    .map(|v| v.id)
                    .collect(),
            },
            CloudCommand::ResumeAll { retry_rejected } => {
                let active = context
                    .scheduler
                    .entries()?
                    .into_iter()
                    .filter(|v| v.enabled || v.running)
                    .map(|v| v.id)
                    .collect::<std::collections::BTreeSet<_>>();
                CloudCommand::Resume {
                    ids: context
                        .uploads
                        .list()?
                        .into_iter()
                        .filter(|v| {
                            !matches!(v.phase, CloudPhase::Complete | CloudPhase::Cancelled)
                                && v.requested != Some(UploadControl::Cancel)
                                && !active.contains(&v.id)
                        })
                        .map(|v| v.id)
                        .collect(),
                    retry_rejected,
                }
            }
            other => other,
        };
        let mut result = CloudResult {
            epoch: epoch.into(),
            accepted: vec![],
            failed: vec![],
            deletion: None,
        };
        match command {
            CloudCommand::Upload { files } => {
                if files.is_empty() || files.len() > 500 {
                    return Err(Error::InvalidInput("invalid upload selection"));
                }
                for file in files {
                    // Echo the selected path, already held by this caller, so
                    // partial failures cannot confuse equal basenames.
                    let label = file.path.to_string_lossy().into_owned();
                    let created = context
                        .uploads
                        .create(
                            &file.path,
                            AttachmentInputs {
                                cover: file.cover,
                                lyric: file.lyric,
                                cue: file.cue,
                            },
                        )
                        .await;
                    context.guard()?;
                    match created {
                        Ok(task) => {
                            context.scheduler.start(&task.id, false)?;
                            result.accepted.push(task.id);
                            context.notify.notify_one();
                        }
                        Err(e) => result.failed.push(CloudFailure {
                            id: label,
                            message: ClientError::from(e).message,
                        }),
                    }
                }
            }
            CloudCommand::Delete { ids, revision } => {
                result.deletion = Some(context.library.delete(&ids, &revision).await?.into());
            }
            CloudCommand::ReconcileDelete { id } => {
                result.deletion = Some(context.library.reconcile_delete(&id).await?.into());
            }
            CloudCommand::Renew { id } => {
                context.uploads.renew_token(&id).await?;
                result.accepted.push(id);
            }
            CloudCommand::ReconcileUpload { id } => {
                if context.uploads.get(&id)?.phase == CloudPhase::PublicationPending {
                    context.uploads.reconcile_publication(&id).await?;
                } else {
                    context.uploads.reconcile_transfer(&id).await?;
                }
                result.accepted.push(id);
            }
            other => {
                let (ids, action, retry) = match other {
                    CloudCommand::Pause { ids } => (ids, "pause", false),
                    CloudCommand::Cancel { ids } => (ids, "cancel", false),
                    CloudCommand::Resume {
                        ids,
                        retry_rejected,
                    } => (ids, "resume", retry_rejected),
                    CloudCommand::Dismiss { ids } => (ids, "dismiss", false),
                    _ => unreachable!(),
                };
                if ids.len() > 5000 {
                    return Err(Error::InvalidInput("invalid upload task selection"));
                }
                for id in &ids {
                    context.uploads.get(id)?;
                }
                for id in ids {
                    let changed = match action {
                        "pause" => context
                            .scheduler
                            .control(&id, UploadControl::Pause)
                            .await
                            .map(|_| ()),
                        "cancel" => context
                            .scheduler
                            .control(&id, UploadControl::Cancel)
                            .await
                            .map(|_| ()),
                        "resume" => context.scheduler.start(&id, retry).map(|_| ()),
                        _ => context.scheduler.dismiss(&id),
                    };
                    context.guard()?;
                    match changed {
                        Ok(()) => result.accepted.push(id),
                        Err(e) => result.failed.push(CloudFailure {
                            id,
                            message: ClientError::from(e).message,
                        }),
                    }
                }
            }
        }
        context.guard()?;
        *context
            .worker_error
            .lock()
            .map_err(|_| Error::LockPoisoned)? = None;
        context.notify.notify_one();
        Ok(result)
    }
}
