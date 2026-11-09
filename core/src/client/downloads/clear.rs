//! One explicit confirmation owns the active inventory read before it opened.
use super::*;
use crate::download::service::{DownloadAction, DownloadStatus};
use std::time::Instant;

const MAX_TASKS: usize = 5000;
const LIFETIME: Duration = Duration::from_secs(5 * 60);

pub(super) struct ClearVisit {
    token: SessionToken,
    receipt: String,
    started: Instant,
    task_ids: Option<Vec<String>>,
}

impl ClearVisit {
    pub(super) fn belongs_to(&self, token: &SessionToken) -> bool {
        &self.token == token
    }

    fn matches(&self, token: &SessionToken, receipt: &str) -> bool {
        self.belongs_to(token) && self.receipt == receipt
    }
}

fn eligible(status: DownloadStatus) -> bool {
    !matches!(
        status,
        DownloadStatus::Complete | DownloadStatus::Cancelled | DownloadStatus::Cancelling
    )
}

impl MusicClient {
    pub async fn download_clear_prepare(&self, epoch: &str) -> Result<DownloadClearPlan> {
        let host = self.downloads.inner.clone();
        let context = host.context(epoch)?;
        let receipt = uuid::Uuid::new_v4().simple().to_string();
        context.session.with_token(&context.token, |_| {
            *host.clear.lock().map_err(|_| Error::LockPoisoned)? = Some(ClearVisit {
                token: context.token.clone(),
                receipt: receipt.clone(),
                started: Instant::now(),
                task_ids: None,
            });
            Ok(())
        })?;
        tokio::task::spawn_blocking(move || {
            let ids = context
                .service
                .list()?
                .into_iter()
                .filter(|task| eligible(task.status))
                .map(|task| task.id)
                .take(MAX_TASKS + 1)
                .collect::<Vec<_>>();
            if ids.len() > MAX_TASKS {
                return Err(Error::InvalidInput(
                    "too many active downloads to clear at once",
                ));
            }
            let count = ids.len();
            context.session.with_token(&context.token, |_| {
                let mut current = host.clear.lock().map_err(|_| Error::LockPoisoned)?;
                let visit = current
                    .as_mut()
                    .filter(|visit| visit.matches(&context.token, &receipt))
                    .ok_or(Error::StaleResponse)?;
                if count == 0 {
                    *current = None;
                } else {
                    visit.task_ids = Some(ids);
                }
                Ok(DownloadClearPlan {
                    epoch: context.token.generation.to_string(),
                    receipt: (count > 0).then_some(receipt),
                    count,
                })
            })
        })
        .await
        .map_err(|_| Error::State("download clear preparation worker stopped"))?
    }

    pub fn download_clear_discard(&self, epoch: &str, receipt: &str) -> Result<()> {
        let host = &self.downloads.inner;
        let token = host.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        host.session.with_token(&token, |_| {
            let mut current = host.clear.lock().map_err(|_| Error::LockPoisoned)?;
            if current
                .as_ref()
                .is_some_and(|visit| visit.matches(&token, receipt))
            {
                *current = None;
            }
            Ok(())
        })
    }
}

pub(super) async fn confirm(
    host: Arc<Host>,
    context: Context,
    receipt: String,
) -> Result<DownloadResult> {
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let ids = context.session.with_token(&context.token, |_| {
            let mut current = host.clear.lock().map_err(|_| Error::LockPoisoned)?;
            let visit = current
                .as_mut()
                .filter(|visit| visit.matches(&context.token, &receipt))
                .ok_or(Error::State("download clear confirmation expired"))?;
            if visit.started.elapsed() >= LIFETIME {
                *current = None;
                return Err(Error::State("download clear confirmation expired"));
            }
            let ids = visit
                .task_ids
                .take()
                .ok_or(Error::State("download clear is not prepared"))?;
            *current = None;
            Ok(ids)
        })?;
        let mut result = DownloadResult {
            epoch: context.token.generation.to_string(),
            ..Default::default()
        };
        for id in ids {
            context.guard()?;
            let task = context.service.get_for(&context.token, &id);
            match task.and_then(|task| {
                if !eligible(task.status) {
                    return Ok(false);
                }
                context
                    .service
                    .request(&id, DownloadAction::Cancel)
                    .map(|_| true)
            }) {
                Ok(true) => result.accepted.push(id),
                Ok(false) => (),
                Err(Error::StaleResponse) => return Err(Error::StaleResponse),
                Err(_) => result.failed.push(DownloadFailure {
                    id,
                    message: "任务状态已变化或清理未完成，请刷新后核对",
                }),
            }
        }
        // The user already confirmed. Wake the worker for any durable cancel
        // intents even if the caller was closed before receiving the result.
        let _runtime = runtime.enter();
        host.kick(&context)?;
        Ok(result)
    })
    .await
    .map_err(|_| Error::State("download clear worker stopped"))?
}
