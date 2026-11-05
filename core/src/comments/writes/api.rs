use super::*;
use crate::{
    Error, Result,
    comments::{CommentApi, CommentResource, ResourceKind},
    protocol::api::{VinylCommentWrite, validate_response},
};
use serde_json::{Value, json};
impl CommentApi {
    pub(super) async fn vinyl_write(
        &self,
        resource: &CommentResource,
        command: &CommentWriteCommand,
        authority: &VinylWriteAuthority<'_>,
    ) -> Result<Value> {
        let CommentWriteCommand::Publish { content, reply_to } = command else {
            return Err(Error::RequestNotSent(Box::new(Error::InvalidInput(
                "vinyl composer accepts only publication",
            ))));
        };
        if resource.kind != ResourceKind::Track || resource.owner.is_some() {
            return Err(Error::RequestNotSent(Box::new(Error::InvalidInput(
                "invalid vinyl comment write scope",
            ))));
        }
        let write = VinylCommentWrite::new(&resource.id, content, reply_to.as_deref())
            .map_err(|error| Error::RequestNotSent(Box::new(error)))?;
        let path = write.path();
        let value = self
            .api
            .request_vinyl_comment_write(write, authority.revision, authority.permission)
            .await?;
        validate_response(path, value, &[200])
    }

    /// Official M438 v/h and M3085 delete, with plain-text M438 defaults.
    pub async fn write(
        &self,
        resource: &CommentResource,
        command: &CommentWriteCommand,
    ) -> Result<Value> {
        let thread = resource.thread()?;
        command.validate()?;
        let (path, payload) = match command {
            CommentWriteCommand::Publish {
                content,
                reply_to: None,
            } => (
                "/api/resource/comments/add",
                json!({"threadId":thread,"content":content,"resourceType":0,"resourceId":0,"expressionPicId":-1,"bubbleId":-1}),
            ),
            CommentWriteCommand::Publish {
                content,
                reply_to: Some(id),
            } => (
                "/api/v1/resource/comments/reply",
                json!({"threadId":thread,"commentId":id,"content":content,"resourceType":0,"resourceId":0}),
            ),
            CommentWriteCommand::Delete { comment_id } => (
                "/api/resource/comments/delete",
                json!({"threadId":thread,"commentId":comment_id}),
            ),
        };
        self.api.call(path, payload, false, &[200]).await
    }
}
