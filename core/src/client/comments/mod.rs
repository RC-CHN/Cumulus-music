mod floor;
mod writes;
use super::{MusicClient, dto::AccountData, mapping};
pub use crate::comments::{
    CommentFloorQuery, CommentQuery, CommentResource, CommentSort, CommentWriteCommand,
    CommentWritePhase,
};
use crate::{
    Error, Result,
    account::{SessionToken, resources::MutationPhase},
    comments::{Comment, CommentLike, Comments},
};
pub use floor::CommentFloorView;
use serde::Serialize;
pub use writes::{CommentWriteAction, CommentWriteView};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentView {
    pub id: String,
    pub parent_id: Option<String>,
    pub content: String,
    pub time: u64,
    pub time_text: String,
    pub liked: Option<bool>,
    pub liked_count: u64,
    pub author: Option<CommentAuthorView>,
    pub replies: Vec<CommentReplyView>,
    pub owned: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentAuthorView {
    pub name: String,
    pub avatar_url: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentReplyView {
    pub id: Option<String>,
    pub content: Option<String>,
    pub author: Option<CommentAuthorView>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentOperationView {
    pub id: String,
    pub desired: bool,
    pub phase: MutationPhase,
    pub observed: Option<CommentView>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentPageView {
    pub resource: CommentResource,
    pub comments: Vec<CommentView>,
    pub hot_comments: Vec<CommentView>,
    pub top_comments: Vec<CommentView>,
    pub current_comment: Option<CommentView>,
    pub total: u64,
    pub more: bool,
    pub more_hot: bool,
    pub next: Option<CommentQuery>,
    pub cached: bool,
    pub operations: Vec<CommentOperationView>,
}
fn author(value: crate::comments::CommentAuthor) -> CommentAuthorView {
    CommentAuthorView {
        name: value.nickname,
        avatar_url: value
            .avatar
            .as_ref()
            .and_then(|v| mapping::artwork(&serde_json::json!(v))),
    }
}
fn comment(value: Comment, uid: &str) -> CommentView {
    CommentView {
        id: value.id,
        parent_id: value.parent_id,
        content: value.content,
        time: value.time,
        time_text: value.time_text,
        liked: value.liked,
        liked_count: value.liked_count,
        owned: value.author.as_ref().is_some_and(|a| a.id == uid),
        author: value.author.map(author),
        replies: value
            .replies
            .into_iter()
            .map(|r| CommentReplyView {
                id: r.id,
                content: r.content,
                author: r.author.map(author),
            })
            .collect(),
    }
}
fn operation(record: CommentLike, uid: &str) -> CommentOperationView {
    CommentOperationView {
        id: record.comment_id,
        desired: record.desired,
        phase: record.phase,
        observed: record.observed.map(|c| comment(c, uid)),
    }
}
impl MusicClient {
    fn comment_token(&self, epoch: &str) -> Result<SessionToken> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(token)
    }
    pub async fn comments_page(
        &self,
        epoch: &str,
        resource: CommentResource,
        query: CommentQuery,
        refresh: bool,
    ) -> Result<AccountData<CommentPageView>> {
        let token = self.comment_token(epoch)?;
        let service = Comments::new(self.session.clone())?;
        let page = if refresh {
            service.page_for(&token, &resource, &query).await?
        } else {
            service
                .cached_for(&token, &resource, &query)?
                .ok_or(Error::State("comment page is not cached"))?
        };
        let operations = service.operations_for(&token, &resource)?;
        self.session.with_token(&token, |uid| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: CommentPageView {
                    resource,
                    comments: page.comments.into_iter().map(|c| comment(c, uid)).collect(),
                    hot_comments: page
                        .hot_comments
                        .into_iter()
                        .map(|c| comment(c, uid))
                        .collect(),
                    top_comments: page
                        .top_comments
                        .into_iter()
                        .map(|c| comment(c, uid))
                        .collect(),
                    current_comment: page.current_comment.map(|c| comment(c, uid)),
                    total: page.total,
                    more: page.more,
                    more_hot: page.more_hot,
                    next: page.next,
                    cached: !refresh,
                    operations: operations.into_iter().map(|r| operation(r, uid)).collect(),
                },
            })
        })
    }
    pub async fn comments_like(
        &self,
        epoch: &str,
        resource: CommentResource,
        id: &str,
        desired: bool,
    ) -> Result<AccountData<CommentOperationView>> {
        let token = self.comment_token(epoch)?;
        let result = Comments::new(self.session.clone())?
            .like_for(&token, &resource, id, desired)
            .await?;
        self.session.with_token(&token, |uid| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: operation(result, uid),
            })
        })
    }
    pub async fn comments_reconcile(
        &self,
        epoch: &str,
        resource: CommentResource,
        id: &str,
    ) -> Result<AccountData<CommentOperationView>> {
        let token = self.comment_token(epoch)?;
        let result = Comments::new(self.session.clone())?
            .reconcile_for(&token, &resource, id)
            .await?;
        self.session.with_token(&token, |uid| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: operation(result, uid),
            })
        })
    }
}
