use super::{model, *};
use crate::{
    Error, Result,
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use std::sync::Arc;
#[derive(Clone)]
pub struct CommentApi {
    pub(super) api: Arc<dyn Api>,
}
impl CommentApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }
    pub async fn page(
        &self,
        resource: &CommentResource,
        query: &CommentQuery,
    ) -> Result<CommentPage> {
        let thread = resource.thread()?;
        query.validate()?;
        let hot = query.sort == CommentSort::Hot;
        let mut params = json!({"limit":query.limit,"offset":query.offset,"beforeTime":query.before_time,"compareUserLocation":query.compare_user_location});
        if !hot {
            params.as_object_mut().expect("object").extend(json!({"commentId":query.comment_id.as_deref().unwrap_or("0"),"composeConcert":query.compose_concert,"markReplied":query.mark_replied,"forceFlatComment":query.force_flat_comment,"showInner":query.show_inner}).as_object().expect("object").clone());
        }
        let path = format!(
            "/api/v1/resource/{}/{thread}",
            if hot { "hotcomments" } else { "comments" }
        );
        let value = self.api.call(&path, params, false, &[200]).await?;
        let mut comments = model::rows(&value[if hot { "hotComments" } else { "comments" }], true)?;
        if comments.len() > query.limit {
            return Err(Error::Response("comment page exceeds request limit"));
        }
        let more = value[if hot { "hasMore" } else { "more" }]
            .as_bool()
            .ok_or(Error::Response("comment pagination flag missing"))?;
        let next = if more {
            let last = comments
                .last()
                .ok_or(Error::Response("comment pagination did not advance"))?;
            Some(CommentQuery {
                offset: query.offset + comments.len(),
                before_time: last.time,
                comment_id: None,
                ..query.clone()
            })
        } else {
            None
        };
        let hot_comments = if hot {
            vec![]
        } else {
            model::rows(&value["hotComments"], false)?
        };
        let top_comments = model::rows(&value["topComments"], false)?;
        let current_comment = value
            .get("currentComment")
            .filter(|v| !v.is_null())
            .map(Comment::from_official)
            .transpose()?;
        if let (Some(id), Some(current)) = (&query.comment_id, &current_comment)
            && id != &current.id
        {
            return Err(Error::Response("located comment identity mismatch"));
        }
        Ok(CommentPage {
            comments: std::mem::take(&mut comments),
            hot_comments,
            top_comments,
            current_comment,
            total: model::number(&value["total"])?,
            more,
            more_hot: value["moreHot"].as_bool().unwrap_or(false),
            next,
        })
    }
    pub async fn target(&self, resource: &CommentResource, id: &str) -> Result<Comment> {
        self.lookup(resource, id)
            .await?
            .ok_or(Error::State("comment is no longer readable"))
    }
    /// A successful empty location is absence of visibility, not proof that a
    /// previous write deleted it. Callers retain acknowledgement separately.
    pub async fn lookup(&self, resource: &CommentResource, id: &str) -> Result<Option<Comment>> {
        identifier(&json!(id))?;
        let page = self
            .page(
                resource,
                &CommentQuery {
                    limit: 1,
                    comment_id: Some(id.into()),
                    ..Default::default()
                },
            )
            .await?;
        Ok(page
            .current_comment
            .into_iter()
            .chain(page.comments)
            .chain(page.hot_comments)
            .chain(page.top_comments)
            .find(|c| c.id == id))
    }
    pub async fn like(&self, resource: &CommentResource, id: &str, desired: bool) -> Result<()> {
        let thread = resource.thread()?;
        let id = identifier(&json!(id))?;
        self.api
            .call(
                if desired {
                    "/api/v1/comment/like"
                } else {
                    "/api/v1/comment/unlike"
                },
                json!({"threadId":thread,"commentId":id}),
                false,
                &[200],
            )
            .await?;
        Ok(())
    }
    pub async fn information(&self, kind: ResourceKind, ids: &[String]) -> Result<Value> {
        let resource_type = match kind {
            ResourceKind::Track => 4,
            ResourceKind::Voice => 1,
            _ => {
                return Err(Error::InvalidInput(
                    "comment information supports tracks and voices",
                ));
            }
        };
        if ids.is_empty() || ids.len() > 100 {
            return Err(Error::InvalidInput("invalid comment information batch"));
        }
        let ids = ids
            .iter()
            .map(|id| identifier(&json!(id)))
            .collect::<Result<Vec<_>>>()?;
        let value = self
            .api
            .call(
                "/api/resource/commentInfo/list",
                json!({"resourceType":resource_type,"resourceIds":serde_json::to_string(&ids)?}),
                false,
                &[200],
            )
            .await?;
        let rows = value["data"]
            .as_array()
            .ok_or(Error::Response("comment information missing"))?;
        if rows.len() != ids.len() {
            return Err(Error::Response("comment information batch is incomplete"));
        }
        for (row, id) in rows.iter().zip(ids) {
            if row["threadId"]
                != (CommentResource {
                    kind,
                    id,
                    owner: None,
                })
                .thread()?
            {
                return Err(Error::Response(
                    "comment information belongs to another resource",
                ));
            }
        }
        Ok(value["data"].clone())
    }
}
