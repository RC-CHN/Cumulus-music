use super::*;
use crate::{
    download::service::{DownloadAction, DownloadStatus},
    protocol::api::identifier,
};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};

struct Retire(Arc<AtomicBool>);
impl Drop for Retire {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl MusicClient {
    pub(in crate::client) async fn start_guarded_song_download(
        &self,
        epoch: &str,
        id: String,
        options: crate::download::service::DownloadOptions,
        permission: impl Fn() -> Result<()> + Send + 'static,
        on_commit: impl FnOnce() + Send + 'static,
    ) -> Result<DownloadResult> {
        let context = self.downloads.inner.context(epoch)?;
        let captured = context.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let _retire = Retire(cancelled.clone());
        let task = tokio::task::spawn_blocking(move || {
            captured.service.create_guarded_song(
                &json!(id),
                options,
                &|| {
                    if super::song_status::download_state(
                        &captured.store,
                        &captured.owner,
                        &id,
                        crate::storage::PlaybackDownloadKind::Song,
                    )? != super::SongDownloadState::None
                    {
                        return Err(Error::State("song already has a download"));
                    }
                    permission()?;
                    if cancelled.load(Ordering::Acquire) {
                        return Err(Error::Cancelled);
                    }
                    Ok(())
                },
                on_commit,
            )
        })
        .await
        .map_err(|_| Error::State("download command worker stopped"))??;
        self.downloads.inner.kick(&context)?;
        Ok(DownloadResult {
            epoch: epoch.into(),
            accepted: vec![task.id],
            ..Default::default()
        })
    }

    pub(in crate::client) async fn start_guarded_song_batch(
        &self,
        epoch: &str,
        batch: GuardedSongBatch,
        permission: impl Fn() -> Result<crate::storage::OperationLock> + Send + 'static,
        on_commit: impl FnOnce(&[String]) + Send + 'static,
    ) -> Result<DownloadResult> {
        let context = self.downloads.inner.context(epoch)?;
        let host = self.downloads.inner.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let _retire = Retire(cancelled.clone());
        let runtime = tokio::runtime::Handle::current();
        let operation_id = batch.receipt.clone();
        let accepted = tokio::task::spawn_blocking(move || {
            let accepted =
                context
                    .service
                    .create_guarded_song_batch(batch, &permission, &cancelled)?;
            // The worker wakes even if the caller lost its response after the
            // transaction committed. The callback also retires the one lease.
            on_commit(&accepted);
            let _runtime = runtime.enter();
            host.kick(&context)?;
            Ok::<_, Error>(accepted)
        })
        .await
        .map_err(|_| Error::State("download batch command worker stopped"))??;
        Ok(DownloadResult {
            epoch: epoch.into(),
            accepted,
            operation_id: Some(operation_id),
            ..Default::default()
        })
    }

    pub async fn download_command(
        &self,
        epoch: &str,
        command: DownloadCommand,
    ) -> Result<DownloadResult> {
        if let DownloadCommand::Preferences { revision, options } = command {
            self.save_download_quality_preferences(epoch, &revision, &options)?;
            return Ok(DownloadResult {
                epoch: epoch.into(),
                ..Default::default()
            });
        }
        let context = self.downloads.inner.context(epoch)?;
        let host = self.downloads.inner.clone();
        match command {
            DownloadCommand::StartVoice { voice_ids, options } => {
                validate_ids(&voice_ids)?;
                let ids = voice_ids
                    .iter()
                    .map(|id| identifier(&json!(id)))
                    .collect::<Result<Vec<_>>>()?;
                let mut result = DownloadResult {
                    epoch: epoch.into(),
                    ..Default::default()
                };
                let mut seen = std::collections::BTreeSet::new();
                for id in ids {
                    if !seen.insert(id.clone()) {
                        continue;
                    }
                    context.guard()?;
                    match context
                        .service
                        .create_voice_default(&json!(id), options.clone())
                        .await
                    {
                        Ok(task) => result.accepted.push(task.id),
                        Err(Error::StaleResponse) => return Err(Error::StaleResponse),
                        Err(_) => result.failed.push(DownloadFailure {
                            id,
                            message: "节目下载未能创建，请检查权限、格式选项和下载目录",
                        }),
                    }
                    host.kick(&context)?;
                }
                context.guard()?;
                Ok(result)
            }
            DownloadCommand::StartMv { id, resolution } => {
                context.guard()?;
                let task = context
                    .service
                    .create_mv_default(&json!(id), resolution)
                    .await?;
                context.guard()?;
                host.kick(&context)?;
                Ok(DownloadResult {
                    epoch: epoch.into(),
                    accepted: vec![task.id],
                    ..Default::default()
                })
            }
            DownloadCommand::StartCloud {
                song_ids,
                revision,
                lyrics,
            } => {
                validate_ids(&song_ids)?;
                let ids = song_ids
                    .iter()
                    .map(|id| identifier(&json!(id)))
                    .collect::<Result<Vec<_>>>()?;
                let snapshot = context
                    .session
                    .run_for(&context.token, |api, _| async move {
                        crate::cloud::CloudApi::new(api).snapshot().await
                    })
                    .await?;
                if snapshot.revision != revision {
                    return Err(Error::QueueChanged);
                }
                let mut entries = snapshot
                    .items
                    .into_iter()
                    .map(|entry| (entry.song_id.clone(), entry))
                    .collect::<std::collections::BTreeMap<_, _>>();
                let mut selected = Vec::new();
                let mut unique = std::collections::BTreeSet::new();
                for id in ids {
                    if unique.insert(id.clone()) {
                        selected.push(entries.remove(&id).ok_or(Error::State(
                            "selected cloud file is no longer in this account",
                        ))?);
                    }
                }
                context.guard()?;
                let captured = context.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let mut result = DownloadResult {
                        epoch: captured.token.generation.to_string(),
                        ..Default::default()
                    };
                    for entry in selected {
                        captured.guard()?;
                        let id = entry.song_id.clone();
                        match captured.service.create_cloud_default(entry, lyrics) {
                            Ok(task) => result.accepted.push(task.id),
                            Err(Error::StaleResponse) => return Err(Error::StaleResponse),
                            Err(_) => result.failed.push(DownloadFailure {
                                id,
                                message: "未能创建云盘下载，请设置目录并检查文件冲突",
                            }),
                        }
                    }
                    Ok(result)
                })
                .await
                .map_err(|_| Error::State("cloud download selection worker stopped"))??;
                context.guard()?;
                host.kick(&context)?;
                Ok(result)
            }
            DownloadCommand::Migration {
                operation_id,
                cancel,
            } => {
                context.directory.get(&operation_id)?;
                host.launch(context, operation_id.clone(), true, cancel, false)?;
                Ok(DownloadResult {
                    epoch: epoch.into(),
                    operation_id: Some(operation_id),
                    ..Default::default()
                })
            }
            DownloadCommand::FileOperation {
                operation_id,
                cancel,
            } => {
                context.files.get(&operation_id)?;
                host.launch(context, operation_id.clone(), false, cancel, false)?;
                Ok(DownloadResult {
                    epoch: epoch.into(),
                    operation_id: Some(operation_id),
                    ..Default::default()
                })
            }
            DownloadCommand::ConfirmClear { receipt } => {
                clear::confirm(host, context, receipt).await
            }
            DownloadCommand::StopOperation { operation_id } => {
                host.stop(&context, &operation_id)?;
                Ok(DownloadResult {
                    epoch: epoch.into(),
                    operation_id: Some(operation_id),
                    ..Default::default()
                })
            }
            DownloadCommand::Control {
                task_ids,
                action: TaskControl::Repair,
            } => {
                validate_ids(&task_ids)?;
                let mut result = DownloadResult {
                    epoch: epoch.into(),
                    ..Default::default()
                };
                for id in task_ids {
                    let repair = context.service.get(&id).and_then(|task| {
                        if task.status != DownloadStatus::Complete {
                            return Err(Error::State("repair expects a completed task"));
                        }
                        host.launch(context.clone(), id.clone(), false, false, true)
                    });
                    match repair {
                        Ok(()) => result.accepted.push(id),
                        Err(Error::StaleResponse) => return Err(Error::StaleResponse),
                        Err(_) => result.failed.push(DownloadFailure {
                            id,
                            message: "该任务正在处理，请稍后重试",
                        }),
                    }
                }
                Ok(result)
            }
            command => {
                let captured = context.clone();
                let result = tokio::task::spawn_blocking(move || execute(&captured, command))
                    .await
                    .map_err(|_| Error::State("download command worker stopped"))??;
                context.guard()?;
                if let Some(id) = &result.operation_id {
                    let migration = context.directory.active()?.as_deref() == Some(id);
                    host.launch(context.clone(), id.clone(), migration, false, false)?;
                }
                host.kick(&context)?;
                Ok(result)
            }
        }
    }
}
fn validate_ids(ids: &[String]) -> Result<()> {
    if ids.is_empty() || ids.len() > 5000 {
        return Err(Error::InvalidInput("select between 1 and 5000 downloads"));
    }
    Ok(())
}
fn execute(context: &Context, command: DownloadCommand) -> Result<DownloadResult> {
    context.guard()?;
    let mut result = DownloadResult {
        epoch: context.token.generation.to_string(),
        ..Default::default()
    };
    match command {
        DownloadCommand::All { action } => {
            match action {
                TaskControl::Pause | TaskControl::Resume => (),
                TaskControl::Cancel => {
                    return Err(Error::InvalidInput(
                        "bulk cancellation requires a prepared confirmation",
                    ));
                }
                TaskControl::Repair => {
                    return Err(Error::InvalidInput("repair requires explicit tasks"));
                }
            }
            let ids: Vec<String> = context
                .service
                .list()?
                .into_iter()
                .filter(|t| match action {
                    TaskControl::Resume => {
                        matches!(t.status, DownloadStatus::Paused | DownloadStatus::Error)
                            && !t.needs_transfer_restart()
                    }
                    TaskControl::Pause => matches!(
                        t.status,
                        DownloadStatus::Queued
                            | DownloadStatus::Authorizing
                            | DownloadStatus::Downloading
                            | DownloadStatus::Processing
                    ),
                    TaskControl::Cancel | TaskControl::Repair => false,
                })
                .map(|t| t.id)
                .collect();
            for chunk in ids.chunks(1000) {
                let next = execute(
                    context,
                    DownloadCommand::Control {
                        task_ids: chunk.to_vec(),
                        action,
                    },
                )?;
                result.accepted.extend(next.accepted);
                result.failed.extend(next.failed);
            }
        }
        DownloadCommand::Start { song_ids, options } => {
            validate_ids(&song_ids)?;
            // Reject malformed IDs before the first task is created.
            let ids = song_ids
                .iter()
                .map(|id| identifier(&json!(id)))
                .collect::<Result<Vec<_>>>()?;
            let mut unique = std::collections::HashSet::new();
            for id in ids.into_iter().filter(|id| unique.insert(id.clone())) {
                context.guard()?;
                match context.service.create_default(&json!(id), options.clone()) {
                    Ok(task) => result.accepted.push(task.id),
                    Err(Error::StaleResponse) => return Err(Error::StaleResponse),
                    Err(_) => result.failed.push(DownloadFailure {
                        id,
                        message: "未能创建任务，请先设置下载目录并检查文件冲突",
                    }),
                }
            }
        }
        DownloadCommand::Control { task_ids, action } => {
            validate_ids(&task_ids)?;
            for id in task_ids {
                context.guard()?;
                let result_task = match action {
                    TaskControl::Pause => context.service.request(&id, DownloadAction::Pause),
                    TaskControl::Cancel => context.service.request(&id, DownloadAction::Cancel),
                    TaskControl::Resume => context.service.enqueue(&id),
                    TaskControl::Repair => return Err(Error::State("repair must use a worker")),
                };
                match result_task {
                    Ok(_) => result.accepted.push(id),
                    Err(Error::StaleResponse) => return Err(Error::StaleResponse),
                    Err(_) => result.failed.push(DownloadFailure {
                        id,
                        message: "任务状态已变化或正在迁移，请刷新后核对",
                    }),
                }
            }
        }
        DownloadCommand::RestartFailedTransfer { task_id } => {
            context
                .service
                .restart_failed_transfer_for(&context.token, &task_id)?;
            result.accepted.push(task_id);
        }
        DownloadCommand::Configure {
            root,
            path_type,
            file_type,
        } => {
            context
                .directory
                .configure(std::path::Path::new(&root), path_type, file_type)?;
        }
        DownloadCommand::Migrate { root } => {
            result.operation_id = Some(
                context
                    .directory
                    .start(std::path::Path::new(&root), None)?
                    .id,
            );
        }
        DownloadCommand::DeleteFiles { task_ids } => {
            validate_ids(&task_ids)?;
            // Each file owns a persistent operation journal. Run sequentially so a
            // large selection cannot create thousands of native workers.
            let mut seen = std::collections::HashSet::new();
            for id in task_ids.into_iter().filter(|id| seen.insert(id.clone())) {
                context.guard()?;
                let operation =
                    context
                        .files
                        .start(&id, crate::download::completed::FileAction::Delete, None);
                match operation.and_then(|op| context.files.resume(&op.id)) {
                    Ok(op) if op.phase == crate::download::completed::FilePhase::Complete => {
                        result.accepted.push(id)
                    }
                    Err(Error::StaleResponse) => return Err(Error::StaleResponse),
                    _ => result.failed.push(DownloadFailure {
                        id,
                        message: "未能删除，请在文件操作中核对状态",
                    }),
                }
            }
        }
        DownloadCommand::File {
            task_id,
            action,
            target,
        } => {
            result.operation_id = Some(
                context
                    .files
                    .start(&task_id, action, target.map(Into::into))?
                    .id,
            );
        }
        DownloadCommand::Rename { task_id, name } => {
            if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
                return Err(Error::InvalidInput("rename accepts only a file name"));
            }
            let task = context.service.get(&task_id)?;
            let extension = task
                .target
                .as_deref()
                .and_then(std::path::Path::extension)
                .and_then(std::ffi::OsStr::to_str)
                .ok_or(Error::State("download has no extension"))?;
            let suffix = format!(".{extension}");
            let name = if name.to_lowercase().ends_with(&suffix.to_lowercase()) {
                name
            } else {
                format!("{name}{suffix}")
            };
            result.operation_id = Some(context.files.rename(&task_id, &name)?.id);
        }
        DownloadCommand::MoveToDirectory { task_id, root } => {
            let task = context.service.get(&task_id)?;
            let name = task
                .target
                .as_deref()
                .and_then(std::path::Path::file_name)
                .ok_or(Error::State("download has no filename"))?;
            result.operation_id = Some(
                context
                    .files
                    .start(
                        &task_id,
                        crate::download::completed::FileAction::Move,
                        Some(std::path::Path::new(&root).join(name)),
                    )?
                    .id,
            );
        }
        _ => return Err(Error::State("operation requires a native worker")),
    }
    context.guard()?;
    Ok(result)
}
