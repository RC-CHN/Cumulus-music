//! M556's ordinary completed-download menu comment information.
use super::{Context, MusicClient};
use crate::{
    Error, Result,
    client::dto::AccountData,
    comments::{CommentApi, ResourceKind},
    download::service::{DownloadOrigin, DownloadStatus},
    protocol::api::identifier,
};
use serde_json::json;
use std::time::Duration;

const MENU_READ_LIMIT: Duration = Duration::from_millis(1_500);

fn download_comment_song(context: &Context, task_id: &str) -> Result<String> {
    let task = context.service.get_for(&context.token, task_id)?;
    if task.status != DownloadStatus::Complete || task.origin != DownloadOrigin::Song {
        return Err(Error::InvalidInput(
            "comment information requires an ordinary completed song download",
        ));
    }
    identifier(&json!(task.song_id))
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
