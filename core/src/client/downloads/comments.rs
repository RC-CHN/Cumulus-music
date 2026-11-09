//! M556 and M622's completed-download comment menus and native identities.
use super::{Context, MusicClient};
use crate::{
    Error, Result,
    client::dto::AccountData,
    comments::{CommentApi, ResourceKind},
    network_diagnostics::DefaultRoute,
};
use std::time::Duration;

const MENU_READ_LIMIT: Duration = Duration::from_millis(1_500);

#[derive(PartialEq, Eq)]
enum DownloadCommentTarget {
    Track(String),
    Voice(String),
}

fn download_comment_target(context: &Context, task_id: &str) -> Result<DownloadCommentTarget> {
    let task = context.service.get_for(&context.token, task_id)?;
    super::resource::catalogue_track_id(&task)
        .map(DownloadCommentTarget::Track)
        .or_else(|| super::resource::episode_comment_id(&task).map(DownloadCommentTarget::Voice))
        .ok_or(Error::InvalidInput(
            "download has no completed comment target",
        ))
}

impl MusicClient {
    pub async fn download_comment_count(
        &self,
        epoch: &str,
        task_id: &str,
        route: DefaultRoute,
    ) -> Result<AccountData<Option<u64>>> {
        let context = self.downloads.inner.context(epoch)?;
        let target = download_comment_target(&context, task_id)?;
        let count = if route == DefaultRoute::Missing {
            // Prefetching a label is optional. Local actions still have the
            // same native source and account checks when routing is absent.
            None
        } else {
            let request_target = &target;
            let count = tokio::time::timeout(
                MENU_READ_LIMIT,
                context
                    .session
                    .run_for(&context.token, |api, _| async move {
                        let comments = CommentApi::new(api);
                        match request_target {
                            DownloadCommentTarget::Track(id) => {
                                comments.count(ResourceKind::Track, id.clone()).await
                            }
                            DownloadCommentTarget::Voice(id) => {
                                comments.downloaded_voice_count(id.clone()).await
                            }
                        }
                    }),
            )
            .await;
            // A failed count may produce a plain label but may not revive a
            // deleted or now foreign row.
            if download_comment_target(&context, task_id)? != target {
                return Err(Error::StaleResponse);
            }
            count.ok().and_then(Result::ok)
        };
        context.guard()?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: count,
        })
    }
}
