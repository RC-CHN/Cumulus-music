//! M556's completed-download comment menu and its catalogue identity.
use super::{Context, MusicClient};
use crate::{
    Error, Result,
    client::dto::AccountData,
    comments::{CommentApi, ResourceKind},
    network_diagnostics::DefaultRoute,
};
use std::time::Duration;

const MENU_READ_LIMIT: Duration = Duration::from_millis(1_500);

fn download_comment_song(context: &Context, task_id: &str) -> Result<String> {
    super::resource::catalogue_track_id(&context.service.get_for(&context.token, task_id)?).ok_or(
        Error::InvalidInput("download has no completed catalogue song comment target"),
    )
}

impl MusicClient {
    pub async fn download_song_comment_count(
        &self,
        epoch: &str,
        task_id: &str,
        route: DefaultRoute,
    ) -> Result<AccountData<Option<u64>>> {
        let context = self.downloads.inner.context(epoch)?;
        let id = download_comment_song(&context, task_id)?;
        let count = if route == DefaultRoute::Missing {
            // Prefetching a label is optional. Local actions still have the
            // same native source and account checks when routing is absent.
            None
        } else {
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
            // A failed count may produce a plain label but may not revive a
            // deleted or now foreign row.
            if download_comment_song(&context, task_id)? != id {
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
