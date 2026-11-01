use super::*;
use crate::{
    cloud::library::CloudDeletionKind,
    download::service::{DownloadStatus, DownloadTask},
};
use std::collections::BTreeMap;

impl MusicClient {
    pub async fn cloud_delete_prepare(
        &self,
        epoch: &str,
        ids: Vec<String>,
        revision: String,
    ) -> Result<CloudRemovalView> {
        checked_ids(&ids)?;
        let context = self.cloud.inner.context(epoch).await?;
        // A prior accepted composition takes precedence over another confirmation.
        let s = store(&context)?;
        let plans = context
            .session
            .with_token(&context.token, |uid| s.values(uid, PLANS))?;
        for value in plans.values() {
            let plan: Plan = serde_json::from_value(value.clone())?;
            plan.validate()?;
            if plan.choice.is_some() && !plan.blocked && plan.ids.iter().any(|id| ids.contains(id))
            {
                let remote = context.library.find_operation(&plan.id)?;
                let unfinished = remote.as_ref().is_none_or(|r| r.phase.pending())
                    || plan.files.iter().any(|f| {
                        matches!(
                            f.state,
                            CloudRemovalFileState::Removing | CloudRemovalFileState::Failed
                        )
                    });
                if unfinished {
                    return self.cloud_delete_view(epoch, &plan.id, false).await;
                }
            }
        }
        if let Some(op) = context
            .library
            .operations()?
            .into_iter()
            .find(|op| op.phase.pending() && op.ids.iter().any(|id| ids.contains(id)))
        {
            return self.cloud_delete_view(epoch, &op.id, false).await;
        }
        let snapshot = context.library.cached()?.ok_or(Error::QueueChanged)?;
        if snapshot.revision != revision {
            return Err(Error::QueueChanged);
        }
        let rows = snapshot
            .items
            .iter()
            .map(|r| (r.song_id.clone(), r))
            .collect::<BTreeMap<_, _>>();
        let mut titles = Vec::new();
        let mut kinds = Vec::new();
        for id in &ids {
            let row = rows.get(id).ok_or(Error::QueueChanged)?;
            titles.push(row.song["name"].as_str().unwrap_or("").to_owned());
            kinds.push(CloudDeletionKind::from_cached(&row.song));
        }
        let download_result = self.cloud_removal_downloads(epoch).await;
        let local = self.local.context().await;
        let captured = context.clone();
        let chosen = ids.clone();
        let names = titles.clone();
        let (files, cue_count, local_issue) = tokio::task::spawn_blocking(move || {
            captured.guard()?;
            match (download_result, local) {
                (Ok(downloads), Ok(local)) => {
                    candidates(&captured, &local.library, &chosen, &names, downloads)
                }
                _ => Ok((vec![], 0, true)),
            }
        })
        .await
        .map_err(|_| Error::State("cloud removal preview worker stopped"))??;
        // Preserve the exact displayed cloud context while local facts were read.
        if context
            .library
            .cached()?
            .is_none_or(|s| s.revision != revision)
        {
            return Err(Error::QueueChanged);
        }
        let plan = Plan {
            id: uuid::Uuid::new_v4().simple().to_string(),
            created_ms: now()?,
            revision,
            ids,
            titles,
            kinds,
            files,
            cue_count,
            local_issue,
            choice: None,
            local_done: false,
            blocked: false,
            issue: None,
        };
        // Bound abandoned, unconfirmed previews. Accepted receipts are retained.
        let mut abandoned = plans
            .into_iter()
            .filter_map(|(id, v)| {
                serde_json::from_value::<Plan>(v)
                    .ok()
                    .filter(|p| p.choice.is_none())
                    .map(|p| (p.created_ms, id))
            })
            .collect::<Vec<_>>();
        abandoned.sort();
        for (_, id) in abandoned.iter().take(abandoned.len().saturating_sub(99)) {
            if let Ok(_lock) = lock(&context, id)
                && load(&context, id)?.is_some_and(|p| p.choice.is_none())
            {
                context
                    .session
                    .with_token(&context.token, |uid| s.delete(uid, PLANS, id))?;
            }
        }
        save(&context, &plan)?;
        view::project(&context, &plan, None, false)
    }
}

fn candidates(
    context: &Context,
    library: &crate::library::LocalLibrary,
    ids: &[String],
    titles: &[String],
    downloads: Vec<DownloadTask>,
) -> Result<(Vec<FileRecord>, usize, bool)> {
    let _index = library.lock()?;
    let cues = match files::cue_sources(library) {
        Ok(v) => v,
        Err(_) => return Ok((vec![], 0, true)),
    };
    let uploads = match context.uploads.list() {
        Ok(v) if v.len() <= 50_000 => v,
        _ => return Ok((vec![], 0, true)),
    };
    let mut downloaded = BTreeMap::new();
    for task in downloads {
        downloaded.entry(task.song_id.clone()).or_insert(task);
    }
    let mut files = Vec::new();
    let mut cue_count = 0;
    let mut issue = false;
    for (cloud_id, title) in ids.iter().zip(titles) {
        context.guard()?;
        let original = uploads.iter().find(|u| {
            u.song_id.as_ref() == Some(cloud_id)
                && matches!(u.phase, CloudPhase::Complete | CloudPhase::Published)
        });
        let candidate = if let Some(upload) = original {
            match context.uploads.removal_source(&upload.id, cloud_id) {
                Ok((path, integrity, cue)) => Some((
                    Origin::Upload {
                        id: upload.id.clone(),
                    },
                    path,
                    integrity,
                    cue,
                )),
                Err(_) => {
                    issue = true;
                    None
                }
            }
        } else if let Some(task) = downloaded.get(cloud_id) {
            if task.status != DownloadStatus::Complete
                || task.file_operation.is_some()
                || task.cleanup_pending
            {
                issue = true;
                None
            } else {
                match (&task.target, &task.output_integrity) {
                    (Some(path), Some(integrity)) => Some((
                        Origin::Download {
                            id: task.id.clone(),
                            operation_id: uuid::Uuid::new_v4().simple().to_string(),
                        },
                        path.clone(),
                        integrity.clone(),
                        false,
                    )),
                    _ => {
                        issue = true;
                        None
                    }
                }
            }
        } else {
            None
        };
        let Some((origin, path, integrity, cue)) = candidate else {
            continue;
        };
        match files::known_cue(&path, &cues) {
            Ok(known) if cue || known => {
                cue_count += 1;
                continue;
            }
            Err(_) => {
                issue = true;
                continue;
            }
            _ => {}
        }
        match files::pin(&path, integrity) {
            Ok(source) => files.push(FileRecord {
                cloud_id: cloud_id.clone(),
                title: title.clone(),
                origin,
                source,
                state: CloudRemovalFileState::Pending,
                issue: None,
            }),
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => issue = true,
        }
    }
    context.guard()?;
    Ok((files, cue_count, issue))
}
