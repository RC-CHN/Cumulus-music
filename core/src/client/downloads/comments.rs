//! M556's completed-download comment menu and its catalogue identity.
use super::{Context, MusicClient};
use crate::{
    Error, Result,
    client::dto::AccountData,
    comments::{CommentApi, ResourceKind},
    download::service::{DownloadOrigin, DownloadStatus, DownloadTask},
    protocol::api::identifier,
};
use serde_json::json;
use std::time::Duration;

const MENU_READ_LIMIT: Duration = Duration::from_millis(1_500);

pub(super) fn comment_track_id(task: &DownloadTask) -> Option<String> {
    if task.status != DownloadStatus::Complete {
        return None;
    }
    match &task.origin {
        DownloadOrigin::Song => identifier(&json!(task.song_id)).ok(),
        DownloadOrigin::Cloud { .. } => {
            // CloudApi records this identity separately from the account's
            // original file ID and omits it for a private songType=1 row.
            let metadata = task.song.as_ref()?;
            let public = identifier(&metadata["_cloud_public_id"]).ok()?;
            let catalog = &metadata["_cloud_catalog_id"];
            if !catalog.is_null() && identifier(catalog).ok().as_ref() != Some(&public) {
                return None;
            }
            Some(public)
        }
        DownloadOrigin::Voice { .. } | DownloadOrigin::Mv { .. } => None,
    }
}

fn download_comment_song(context: &Context, task_id: &str) -> Result<String> {
    comment_track_id(&context.service.get_for(&context.token, task_id)?).ok_or(Error::InvalidInput(
        "download has no completed catalogue song comment target",
    ))
}

impl MusicClient {
    pub async fn download_song_comment_count(
        &self,
        epoch: &str,
        task_id: &str,
    ) -> Result<AccountData<Option<u64>>> {
        let context = self.downloads.inner.context(epoch)?;
        let id = download_comment_song(&context, task_id)?;
        let request_id = id.clone();
        let count = tokio::time::timeout(
            MENU_READ_LIMIT,
            context
                .session
                .run_for(&context.token, |api, _| async move {
                    CommentApi::new(api)
                        .count(ResourceKind::Track, request_id)
                        .await
                }),
        )
        .await;
        // A harmless network or response failure may produce a plain label,
        // but it must not make a now foreign or deleted task visible again.
        if download_comment_song(&context, task_id)? != id {
            return Err(Error::StaleResponse);
        }
        context.guard()?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: count.ok().and_then(Result::ok),
        })
    }
}
