use super::*;
impl MusicClient {
    pub async fn cloud_command(&self, epoch: &str, command: CloudCommand) -> Result<CloudResult> {
        let context = self.cloud.inner.context(epoch).await?;
        let needs_capacity = matches!(
            command,
            CloudCommand::Upload { .. }
                | CloudCommand::UploadPlanned { .. }
                | CloudCommand::RetryEncoding { .. }
                | CloudCommand::Resume { .. }
                | CloudCommand::ResumeAll { .. }
        );
        let capacity_full = needs_capacity && context.library.cached()?.is_some_and(|snapshot| {
            matches!((snapshot.size, snapshot.max_size), (Some(size), Some(max)) if max > 0 && size >= max)
        });
        if capacity_full && !matches!(command, CloudCommand::UploadPlanned { .. }) {
            return Err(Error::State("cloud capacity exhausted"));
        }
        let bulk_tasks = matches!(
            command,
            CloudCommand::PauseAll | CloudCommand::ResumeAll { .. }
        );
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
                    .filter(|v| v.enabled || v.running || v.removing)
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
                                && v.error
                                    .as_ref()
                                    .is_none_or(|error| error.kind != "encoding_failed")
                                && !v.blocked_text()
                                && !v.unresolved_nos_boundary()
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
            not_started: vec![],
            upload_limit: None,
            deletion: None,
        };
        match command {
            CloudCommand::Upload { files } => {
                add_files(
                    &context,
                    files.into_iter().map(|file| (None, file)).collect(),
                    true,
                    &mut result,
                )
                .await?;
            }
            CloudCommand::UploadPlanned { files } => {
                let mut ids = std::collections::BTreeSet::new();
                for file in &files {
                    if file.request_id.len() != 32
                        || !file
                            .request_id
                            .bytes()
                            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                        || !ids.insert(&file.request_id)
                    {
                        return Err(Error::InvalidInput("invalid cloud creation request IDs"));
                    }
                }
                add_files(
                    &context,
                    files
                        .into_iter()
                        .map(|file| (Some(file.request_id), file.input))
                        .collect(),
                    !capacity_full,
                    &mut result,
                )
                .await?;
            }
            CloudCommand::Delete { ids, revision } => {
                result.deletion = Some(context.library.delete(&ids, &revision).await?.into());
            }
            CloudCommand::ReconcileDelete { id } => {
                result.deletion = Some(context.library.reconcile_delete(&id).await?.into());
            }
            CloudCommand::RetryEncoding { id } => {
                context.uploads.retry_encoding(&id)?;
                context.scheduler.start(&id, false)?;
                result.accepted.push(id);
            }
            CloudCommand::Renew { id } => {
                context.uploads.renew_token(&id).await?;
                result.accepted.push(id);
            }
            CloudCommand::OmitTextAttachment {
                id,
                attachment,
                expected_revision,
            } => {
                context
                    .scheduler
                    .omit_text_attachment(&id, attachment, &expected_revision)?;
                context.guard()?;
                result.accepted.push(id);
                return Ok(result);
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
                    CloudCommand::RemoveUploads { ids, retry } => (ids, "remove", retry),
                    _ => unreachable!(),
                };
                if ids.len() > if bulk_tasks { 50_000 } else { 5000 }
                    || ids.iter().collect::<std::collections::BTreeSet<_>>().len() != ids.len()
                {
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
                        "remove" => context.scheduler.remove(&id, retry).await.map(|_| ()),
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
async fn add_files(
    context: &Context,
    files: Vec<(Option<String>, CloudUploadInput)>,
    allow_new: bool,
    result: &mut CloudResult,
) -> Result<()> {
    if files.is_empty() || files.len() > 500 {
        return Err(Error::InvalidInput("invalid upload selection"));
    }
    let mut new_task = false;
    let mut over_limit = false;
    for (request_id, file) in files {
        let label = file.path.to_string_lossy().into_owned();
        let attachments = AttachmentInputs {
            cover: file.cover,
            lyric: file.lyric,
            cue: file.cue,
        };
        let created = if let Some(request_id) = request_id {
            context
                .uploads
                .create_once(&request_id, &file.path, attachments, allow_new)
                .await
        } else {
            context
                .uploads
                .create(&file.path, attachments)
                .await
                .map(|task| (task, true))
        };
        context.guard()?;
        match created {
            Ok((task, fresh)) => {
                new_task |= fresh;
                // An old receipt is a read, never implicit resume/retry. In
                // particular keep old pause/cancel and unknown remote intents.
                let scheduled = if fresh {
                    context.scheduler.start(&task.id, false).map(|_| ())
                } else {
                    Ok(())
                };
                let retained = !fresh
                    && task.phase != CloudPhase::Complete
                    && !context
                        .scheduler
                        .entry(&task.id)?
                        .is_some_and(|entry| entry.enabled || entry.running);
                result.accepted.push(task.id);
                context.notify.notify_one();
                if retained {
                    result.not_started.push(CloudFailure {
                        id: label,
                        message: "已登记任务保留原状态，未重新启动",
                    });
                } else if let Err(e) = scheduled {
                    result.not_started.push(CloudFailure {
                        id: label,
                        message: ClientError::from(e).message,
                    });
                }
            }
            Err(e) => {
                over_limit |= matches!(e.root(), Error::CloudUploadTooLarge);
                result.failed.push(CloudFailure {
                    id: label,
                    message: ClientError::from(e).message,
                });
            }
        }
    }
    if over_limit {
        result.upload_limit = Some(if new_task {
            CloudUploadLimit::Partial
        } else {
            CloudUploadLimit::NoNewTasks
        });
    }
    Ok(())
}
