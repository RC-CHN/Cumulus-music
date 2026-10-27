use super::*;
use crate::{
    client::mapping,
    download::{
        completed::FilePhase,
        service::{DownloadStatus, DownloadTask},
        transfer::TransferJob,
    },
};
use serde_json::json;
impl Host {
    pub(super) fn view(&self, context: &Context, query: DownloadQuery) -> Result<DownloadsView> {
        mapping::page(query.offset, query.limit)?;
        if query.keyword.len() > 1000 {
            return Err(Error::InvalidInput("download keyword too long"));
        }
        context.guard()?;
        // Observe completion before reading the data it committed. A worker
        // can finish while this view is assembled: sampling jobs/operations
        // after tasks could claim "finished" alongside a stale pre-rename path.
        let jobs = self
            .jobs
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .values()
            .filter(|j| j.token == context.token)
            .map(|j| {
                Ok(DownloadJobView {
                    id: j.id.clone(),
                    kind: j.kind,
                    running: j.running.load(Ordering::Acquire),
                    error: *j.error.lock().map_err(|_| Error::LockPoisoned)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let directory = context.directory.settings()?;
        let migration = context
            .directory
            .active()?
            .map(|id| context.directory.get(&id))
            .transpose()?;
        let operations = context
            .files
            .list()?
            .into_iter()
            .filter(|o| {
                !matches!(o.phase, FilePhase::Complete | FilePhase::Cancelled)
                    || o.old_work_retained
                    || o.files.iter().any(|f| f.replacement_retained)
            })
            .collect();
        let options = context
            .store
            .get(&context.owner, PREFERENCES, "options")?
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let tasks = context.service.list()?;
        let completed_count = tasks
            .iter()
            .filter(|t| t.status == DownloadStatus::Complete)
            .count();
        let active_count = tasks
            .iter()
            .filter(|t| {
                !matches!(
                    t.status,
                    DownloadStatus::Complete | DownloadStatus::Cancelled
                )
            })
            .count();
        let mut values = Vec::new();
        let keyword = query.keyword.trim().to_lowercase();
        for task in tasks {
            if task.status == DownloadStatus::Cancelled
                || (task.status == DownloadStatus::Complete)
                    != (query.tab == DownloadTab::Completed)
            {
                continue;
            }
            let track = task
                .song
                .as_ref()
                .map(mapping::track)
                .transpose()?
                .unwrap_or(mapping::track(
                    &json!({"id":task.song_id,"name":format!("歌曲 {}",task.song_id)}),
                )?);
            if !keyword.is_empty()
                && !format!("{} {} {}", track.name, track.artists.join(" "), track.album)
                    .to_lowercase()
                    .contains(&keyword)
            {
                continue;
            }
            let raw: Option<TransferJob> = context
                .store
                .get(&context.owner, "media_transfers_v1", &task.id)?
                .map(serde_json::from_value)
                .transpose()?;
            let total = task
                .output_integrity
                .as_ref()
                .map(|v| v.bytes)
                .or_else(|| task.resource.as_ref().and_then(|v| v["size"].as_u64()));
            let bytes = if task.status == DownloadStatus::Complete {
                total.unwrap_or(task.bytes)
            } else {
                task.bytes.max(raw.as_ref().map_or(0, |r| r.bytes))
            };
            let progress = if task.status == DownloadStatus::Complete {
                Some(1.0)
            } else {
                total
                    .filter(|n| *n > 0)
                    .map(|n| (bytes as f64 / n as f64 * 0.95).clamp(0.0, 0.95))
            };
            let actual_quality = task
                .resource
                .as_ref()
                .and_then(|r| r["level"].as_str())
                .and_then(|s| crate::membership::Quality::parse(s).ok())
                .map(|q| q.as_str().to_owned());
            let error = task_error(&task);
            values.push(DownloadTaskView {
                id: task.id,
                source: if task.origin.is_cloud() {
                    "cloud"
                } else if task.origin.is_mv() {
                    "mv"
                } else if task.origin.is_voice() {
                    "voice"
                } else {
                    "song"
                },
                track,
                status: task.status,
                phase: task.phase,
                bytes,
                total,
                progress,
                requested_quality: task.options.quality.as_str().into(),
                actual_quality,
                requested_resolution: match task.origin {
                    crate::download::service::DownloadOrigin::Mv { resolution } => Some(resolution),
                    _ => None,
                },
                actual_resolution: if task.origin.is_mv() {
                    task.resource
                        .as_ref()
                        .and_then(|r| r["r"].as_u64())
                        .and_then(|r| u32::try_from(r).ok())
                } else {
                    None
                },
                encrypted: task.encrypted,
                path: task
                    .target
                    .and_then(|p| p.into_os_string().into_string().ok()),
                created_ms: task.created_ms,
                completed_ms: task.completed_ms,
                cleanup_pending: task.cleanup_pending,
                file_operation: task.file_operation,
                lyrics_status: task.lyrics_status,
                error,
            });
        }
        values.sort_by(|a, b| {
            let order = match query.sort {
                DownloadSort::Name => a.track.name.cmp(&b.track.name),
                DownloadSort::Artist => a.track.artists.cmp(&b.track.artists),
                DownloadSort::Album => a.track.album.cmp(&b.track.album),
                DownloadSort::Size => a.total.cmp(&b.total),
                DownloadSort::Completed => a
                    .completed_ms
                    .cmp(&b.completed_ms)
                    .then(a.created_ms.cmp(&b.created_ms)),
            };
            (if query.descending {
                order.reverse()
            } else {
                order
            })
            .then(a.id.cmp(&b.id))
        });
        let total = values.len();
        let values = values
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .collect();
        context.guard()?;
        Ok(DownloadsView {
            epoch: context.token.generation.to_string(),
            tasks: values,
            total,
            active_count,
            completed_count,
            offset: query.offset,
            directory,
            options,
            migration,
            operations,
            jobs,
        })
    }
}
fn task_error(task: &DownloadTask) -> Option<&'static str> {
    if task.cleanup_pending {
        return Some("音频已保留，歌词或临时文件仍需完成处理");
    }
    if task.status == DownloadStatus::Paused || task.last_error.is_none() {
        return None;
    }
    let raw = task.last_error.as_deref().unwrap_or_default();
    Some(
        if raw.contains("already exists") || raw.contains("destination exists") {
            "目标文件已存在，请检查文件后再继续"
        } else if task
            .service_code
            .as_ref()
            .is_some_and(|v| v == 403 || v == 401 || v == 301)
        {
            "服务未允许此次下载，请检查登录与资源权限"
        } else if raw.contains("cloud original changed")
            || raw.contains("cloud download size differs")
        {
            "云盘原文件已变化，请刷新云盘后重新创建下载"
        } else if raw.contains("cloud download is no longer") {
            "该文件已不在当前账号的云盘中"
        } else if raw.contains("space") || raw.contains("Space") {
            "磁盘空间不足，请腾出空间后继续"
        } else if raw.contains("trial") {
            "该资源仅返回试听，无法下载完整歌曲"
        } else {
            "下载未完成，可重试；已有文件和断点已保留"
        },
    )
}
