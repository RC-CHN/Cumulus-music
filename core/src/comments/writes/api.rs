use super::*;
use crate::{
    Result,
    comments::{CommentApi, CommentResource},
};
use serde_json::{Value, json};
impl CommentApi {
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
