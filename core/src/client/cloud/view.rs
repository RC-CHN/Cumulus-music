use super::*;
use crate::cloud::oss::OssPending;
use serde_json::Value;
use std::{cmp::Ordering, collections::HashMap};
pub(super) fn project(context: &Context, query: CloudQuery) -> Result<CloudPageView> {
    context.guard()?;
    // Sample worker facts before task/catalog data: a completed worker implies
    // all previous native commits are already visible to subsequent reads.
    let entries = context
        .scheduler
        .entries()?
        .into_iter()
        .map(|v| (v.id.clone(), v))
        .collect::<HashMap<_, _>>();
    let tasks = context.uploads.list()?;
    let snapshot = context.library.cached()?;
    let keyword = query.keyword.trim().to_lowercase();
    let uploaded_count = snapshot.as_ref().map_or(0, |s| s.items.len());
    let mut tracks = catalog(snapshot.as_ref(), &query)?;
    let mut uploads = tasks
        .into_iter()
        .filter(|t| !entries.get(&t.id).is_some_and(|e| e.hidden))
        .map(|task| upload(task, &entries))
        .collect::<Vec<_>>();
    let uploading_count = uploads
        .iter()
        .filter(|v| !matches!(v.phase, CloudPhase::Complete | CloudPhase::Cancelled))
        .count();
    let history_count = uploads
        .iter()
        .filter(|v| matches!(v.phase, CloudPhase::Complete | CloudPhase::Cancelled))
        .count();
    uploads.retain(|v| {
        let terminal = matches!(v.phase, CloudPhase::Complete | CloudPhase::Cancelled);
        (match query.tab {
            CloudTab::Uploading => !terminal,
            CloudTab::History => terminal,
            CloudTab::Uploaded => true,
        }) && (v.title.to_lowercase().contains(&keyword)
            || v.file_name.to_lowercase().contains(&keyword))
    });
    uploads.sort_by(|a, b| {
        entries
            .get(&b.id)
            .map(|v| v.order)
            .cmp(&entries.get(&a.id).map(|v| v.order))
            .then(Ordering::Equal)
    });
    let total = match query.tab {
        CloudTab::Uploaded => tracks.len(),
        CloudTab::Uploading | CloudTab::History => uploads.len(),
    };
    match query.tab {
        CloudTab::Uploaded => {
            tracks = tracks
                .into_iter()
                .skip(query.offset)
                .take(query.limit)
                .collect();
            uploads.clear();
        }
        CloudTab::Uploading | CloudTab::History => {
            uploads = uploads
                .into_iter()
                .skip(query.offset)
                .take(query.limit)
                .collect();
            tracks.clear();
        }
    }
    let mut operations = context.library.operations()?;
    operations.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then(b.id.cmp(&a.id)));
    let deletions = operations.into_iter().take(30).map(Into::into).collect();
    context.guard()?;
    Ok(CloudPageView {
        epoch: context.token.generation.to_string(),
        revision: snapshot.as_ref().map(|s| s.revision.clone()),
        updated_ms: snapshot.as_ref().map(|s| s.updated_ms),
        size: snapshot.as_ref().and_then(|s| s.size),
        max_size: snapshot.as_ref().and_then(|s| s.max_size),
        uploaded_count,
        uploading_count,
        history_count,
        total,
        offset: query.offset,
        tracks,
        uploads,
        deletions,
        worker_error: *context
            .worker_error
            .lock()
            .map_err(|_| Error::LockPoisoned)?,
    })
}
fn upload(task: CloudView, entries: &HashMap<String, UploadSchedule>) -> CloudUploadView {
    let state = entries.get(&task.id);
    let enabled = state.is_some_and(|s| s.enabled);
    let running = state.is_some_and(|s| s.running);
    let terminal = matches!(task.phase, CloudPhase::Complete | CloudPhase::Cancelled);
    let unknown = task
        .error
        .as_ref()
        .is_some_and(|e| e.kind == "write_outcome_unknown")
        || matches!(&task.transfer,Some(CloudTransfer::Oss(v)) if matches!(v.pending,Some(OssPending::Initiate|OssPending::Put|OssPending::Complete|OssPending::Abort)));
    // An active chunk also has durable intent. It is not a failed outcome
    // until its worker has stopped without an acknowledgement.
    let unresolved_nos = !terminal && !running && task.unresolved_nos_boundary();
    let pending = matches!(
        task.phase,
        CloudPhase::AllocationPending
            | CloudPhase::MetadataPending
            | CloudPhase::PublicationPending
    ) || task
        .attachments
        .iter()
        .any(|a| a.phase == AttachmentPhase::AllocationPending);
    let removing = state.is_some_and(|state| state.removing);
    let cancellable = !unknown && !pending && task.phase != CloudPhase::Published;
    let cancelling = task.requested == Some(UploadControl::Cancel);
    let reconcile = task.phase == CloudPhase::PublicationPending
        || matches!(&task.transfer,Some(CloudTransfer::Oss(v)) if matches!(v.pending,Some(OssPending::Put|OssPending::Complete)));
    let renew = (task.phase == CloudPhase::AudioUploading && task.channel == Some(1))
        || (task.phase == CloudPhase::AttachmentsUploading
            && task
                .attachments
                .iter()
                .any(|a| a.phase == AttachmentPhase::Uploading));
    let child_error = match &task.transfer {
        Some(CloudTransfer::Nos(v)) => v.error.clone(),
        Some(CloudTransfer::Oss(v)) => v.error.clone(),
        _ => None,
    };
    let error = unresolved_nos
        .then(|| "nos_boundary_unverified".to_owned())
        .or_else(|| state.and_then(|s| s.error.clone()))
        .or_else(|| task.error.as_ref().map(|e| e.kind.clone()))
        .or(child_error)
        .filter(|error| !terminal && !(running && error == "write_outcome_unknown"));
    let encoding_failed =
        task.phase == CloudPhase::Failed && error.as_deref() == Some("encoding_failed");
    let label = if terminal {
        if task.phase == CloudPhase::Complete {
            "已上传"
        } else {
            "已取消"
        }
    } else if unresolved_nos && !cancelling && !removing {
        "无法核对上传结果"
    } else if (unknown || reconcile) && !running {
        "等待核对"
    } else if removing {
        "正在移除"
    } else if cancelling {
        "取消待确认"
    } else if task.requested == Some(UploadControl::Pause) {
        "已暂停"
    } else if error.is_some() {
        "上传未完成"
    } else {
        match task.phase {
            CloudPhase::Encoding => "正在转码",
            CloudPhase::ReadyToPublish | CloudPhase::PublicationPending | CloudPhase::Published => {
                "正在保存"
            }
            _ if !enabled && !running => "等待继续",
            CloudPhase::Queued | CloudPhase::Checked | CloudPhase::Allocated => "等待上传",
            _ => "正在上传",
        }
    };
    let blocked_text = task.blocked_text();
    let omittable_attachment = state
        .filter(|s| {
            !s.enabled
                && !s.running
                && !s.hidden
                && !s.removing
                && !unknown
                && !unresolved_nos
                && !cancelling
        })
        .and_then(|s| {
            task.omittable_text
                .map(|attachment| CloudAttachmentOmission {
                    attachment,
                    revision: s.revision.to_string(),
                })
        });
    CloudUploadView {
        id: task.id,
        title: task.title,
        artists: task.artists,
        album: task.album,
        file_name: task
            .source
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .into(),
        phase: task.phase,
        bytes: task.bytes,
        total: task.total,
        label,
        enabled,
        running,
        error,
        can_resume: !blocked_text
            && !encoding_failed
            && !terminal
            && !unknown
            && !unresolved_nos
            && !pending
            && !reconcile
            && !removing
            && !cancelling
            && !enabled
            && !running,
        can_pause: !terminal && (enabled || running) && !cancelling && !removing,
        can_cancel: !terminal && cancellable && !cancelling,
        removal_pending: removing,
        can_remove: !removing && (terminal || cancellable),
        can_retry_remove: removing && !running && (terminal || cancellable),
        can_retry_encoding: encoding_failed
            && !removing
            && !running
            && !cancelling
            && task.song_id.is_some(),
        can_reconcile: reconcile && !running,
        can_renew: renew && !unresolved_nos && !running && !removing && !cancelling,
        can_dismiss: terminal,
        attachments: task.attachments,
        omittable_attachment,
    }
}

pub(super) fn catalog(
    snapshot: Option<&crate::cloud::library::CloudSnapshot>,
    query: &CloudQuery,
) -> Result<Vec<CloudTrackView>> {
    let keyword = query.keyword.trim().to_lowercase();
    let mut tracks = snapshot
        .map(|s| {
            s.items
                .iter()
                .map(|entry| {
                    let track = super::super::mapping::track(&entry.song)?;
                    Ok((
                        CloudTrackView {
                            deletion_kind: crate::cloud::library::CloudDeletionKind::from_cached(
                                &entry.song,
                            ),
                            public_song_id: (entry.song["privilege"]["st"]
                                .as_i64()
                                .is_none_or(|status| status >= 0)
                                && entry.song["privilege"]["status"]
                                    .as_i64()
                                    .is_none_or(|status| status >= 0))
                            .then(|| {
                                crate::protocol::api::identifier(&entry.song["_cloud_public_id"])
                                    .ok()
                            })
                            .flatten(),
                            track,
                            file_name: entry.file_name.clone(),
                            file_size: entry.file_size,
                            added_at: entry.added_at,
                            bitrate: entry.bitrate,
                            format: std::path::Path::new(&entry.file_name)
                                .extension()
                                .and_then(|s| s.to_str())
                                .unwrap_or("")
                                .to_lowercase(),
                        },
                        &entry.song,
                    ))
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    tracks.retain(|(row, _)| {
        [
            row.track.name.as_str(),
            row.track.album.as_str(),
            row.file_name.as_str(),
        ]
        .into_iter()
        .chain(row.track.artists.iter().map(String::as_str))
        .any(|s| s.to_lowercase().contains(&keyword))
    });
    if matches!(query.sort, CloudSort::Natural) {
        return Ok(tracks.into_iter().map(|(row, _)| row).collect());
    }
    let order = crate::cloud::sort::CloudTextOrder::new()?;
    // Encode once per row rather than allocating on every comparison. The
    // complete account list, not the visible 200 rows, defines playback order.
    let keys = tracks
        .iter()
        .map(|(row, song)| {
            match query.sort {
                CloudSort::Title => text(song, row, "title"),
                CloudSort::Artist => text(song, row, "artist"),
                CloudSort::Album => text(song, row, "album"),
                CloudSort::Format => &row.format,
                _ => "",
            }
            .encode_utf16()
            .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let indices = crate::cloud::sort::stable_order(tracks.len(), |i, j| {
        let (a, _) = &tracks[i];
        let (b, _) = &tracks[j];
        let result = match query.sort {
            CloudSort::Natural => Ordering::Equal,
            CloudSort::Uploaded => a.added_at.cmp(&b.added_at),
            CloudSort::Title | CloudSort::Artist | CloudSort::Album | CloudSort::Format => {
                order.compare_utf16(&keys[i], &keys[j])
            }
            CloudSort::Size => a.file_size.cmp(&b.file_size),
        };
        if query.descending {
            result.reverse()
        } else {
            result
        }
    });
    let mut rows = tracks
        .into_iter()
        .map(|(row, _)| Some(row))
        .collect::<Vec<_>>();
    Ok(indices
        .into_iter()
        .map(|i| rows[i].take().expect("sort preserves unique indices"))
        .collect())
}
fn text<'a>(song: &'a Value, row: &'a CloudTrackView, key: &str) -> &'a str {
    song["_cloud_sort"][key]
        .as_str()
        .unwrap_or_else(|| match key {
            "title" => &row.track.name,
            "album" => &row.track.album,
            _ => row.track.artists.first().map(String::as_str).unwrap_or(""),
        })
}
