use super::{AccountData, CommentView, MusicClient, comment};
use crate::{
    Error, Result,
    comments::{CommentFloorQuery, CommentResource, Comments},
};
use serde::Serialize;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentFloorView {
    pub resource: CommentResource,
    pub query: CommentFloorQuery,
    pub owner: Option<CommentView>,
    pub current_comment: Option<CommentView>,
    pub comments: Vec<CommentView>,
    pub best_comments: Vec<CommentView>,
    pub total: u64,
    pub more: bool,
    pub next: Option<CommentFloorQuery>,
    pub cached: bool,
}
impl MusicClient {
    pub async fn comments_floor(
        &self,
        epoch: &str,
        resource: CommentResource,
        query: CommentFloorQuery,
        refresh: bool,
    ) -> Result<AccountData<CommentFloorView>> {
        let token = self.comment_token(epoch)?;
        let service = Comments::new(self.session.clone())?;
        let page = if refresh {
            service.floor_for(&token, &resource, &query).await?
        } else {
            service
                .cached_floor_for(&token, &resource, &query)?
                .ok_or(Error::State("comment floor is not cached"))?
        };
        self.session.with_token(&token, |uid| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: CommentFloorView {
                    resource,
                    query,
                    owner: page.owner.map(|v| comment(v, uid)),
                    current_comment: page.current.map(|v| comment(v, uid)),
                    comments: page.comments.into_iter().map(|v| comment(v, uid)).collect(),
                    best_comments: page
                        .best_comments
                        .into_iter()
                        .map(|v| comment(v, uid))
                        .collect(),
                    total: page.total,
                    more: page.more,
                    next: page.next,
                    cached: !refresh,
                },
            })
        })
    }
}
