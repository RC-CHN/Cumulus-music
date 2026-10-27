//! Floor endpoint is absent from the recovered desktop registry. Parameters
//! are sourced from archived API-Reborn and checked against a real PC response.
use super::*;
use crate::{Error, Result, account::SessionToken, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
pub(super) const FLOORS: &str = "comment-floors-v1";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommentFloorQuery {
    pub parent_id: String,
    pub time: i64,
    pub limit: usize,
}
impl CommentFloorQuery {
    pub fn first(parent_id: String) -> Self {
        Self {
            parent_id,
            time: -1,
            limit: 20,
        }
    }
    pub fn validate(&self) -> Result<()> {
        identifier(&json!(self.parent_id))?;
        if self.parent_id.len() > 128
            || !(1..=100).contains(&self.limit)
            || !(self.time == -1 || (1..=9_007_199_254_740_991).contains(&self.time))
        {
            return Err(Error::InvalidInput("invalid comment floor cursor"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommentFloorPage {
    pub owner: Option<Comment>,
    pub current: Option<Comment>,
    pub comments: Vec<Comment>,
    pub best_comments: Vec<Comment>,
    pub total: u64,
    pub more: bool,
    pub next: Option<CommentFloorQuery>,
}
impl CommentApi {
    pub async fn floor(
        &self,
        resource: &CommentResource,
        query: &CommentFloorQuery,
    ) -> Result<CommentFloorPage> {
        query.validate()?;
        let value=self.api.call("/api/resource/comment/floor/get",json!({"threadId":resource.thread()?,"parentCommentId":query.parent_id,"time":query.time,"limit":query.limit}),false,&[200]).await?;
        let data = &value["data"];
        let owner = data
            .get("ownerComment")
            .filter(|v| !v.is_null())
            .map(Comment::from_official)
            .transpose()?;
        if owner.as_ref().is_some_and(|v| v.id != query.parent_id) {
            return Err(Error::Response("comment floor root identity mismatch"));
        }
        let comments = model::rows(&data["comments"], true)?;
        let best_comments = model::rows(&data["bestComments"], false)?;
        let current = data
            .get("currentComment")
            .filter(|v| !v.is_null())
            .map(Comment::from_official)
            .transpose()?;
        if comments.len() > query.limit {
            return Err(Error::Response("comment floor page exceeds limit"));
        }
        for c in comments.iter().chain(&best_comments) {
            if c.parent_id.as_ref() != Some(&query.parent_id) {
                return Err(Error::Response(
                    "comment floor child belongs to another parent",
                ));
            }
        }
        if current.as_ref().is_some_and(|c| {
            c.id != query.parent_id && c.parent_id.as_ref() != Some(&query.parent_id)
        }) {
            return Err(Error::Response("current floor comment identity mismatch"));
        }
        let more = data["hasMore"]
            .as_bool()
            .ok_or(Error::Response("comment floor pagination flag missing"))?;
        let next = if more {
            let time = data["time"]
                .as_i64()
                .ok_or(Error::Response("comment floor cursor missing"))?;
            let next = CommentFloorQuery {
                time,
                ..query.clone()
            };
            next.validate()?;
            if comments.is_empty() || time <= query.time {
                return Err(Error::Response("comment floor pagination did not advance"));
            }
            Some(next)
        } else {
            None
        };
        Ok(CommentFloorPage {
            owner,
            current,
            comments,
            best_comments,
            total: model::number(&data["totalCount"])?,
            more,
            next,
        })
    }
}
#[derive(Serialize, Deserialize)]
struct CachedFloor {
    resource: CommentResource,
    query: CommentFloorQuery,
    revision: Option<Value>,
    page: CommentFloorPage,
}
fn key(resource: &CommentResource, query: &CommentFloorQuery) -> Result<String> {
    resource.thread()?;
    query.validate()?;
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(
        resource, query,
    ))?)))
}
impl Comments {
    pub async fn floor_for(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
        query: &CommentFloorQuery,
    ) -> Result<CommentFloorPage> {
        let key = key(resource, query)?;
        let revision = self.revision(token, resource)?;
        let page = self
            .session
            .run_for(token, |api, _| async move {
                CommentApi::new(api).floor(resource, query).await
            })
            .await?;
        let cached = CachedFloor {
            resource: resource.clone(),
            query: query.clone(),
            revision: revision.clone(),
            page,
        };
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                if tx.get(owner, cache::REVISIONS, &resource.thread()?)? != revision {
                    return Err(Error::QueueChanged);
                }
                tx.put(owner, FLOORS, &key, &serde_json::to_value(&cached)?)?;
                Ok(cached.page)
            })
        })
    }
    pub fn cached_floor_for(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
        query: &CommentFloorQuery,
    ) -> Result<Option<CommentFloorPage>> {
        let key = key(resource, query)?;
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                let Some(value) = tx.get(owner, FLOORS, &key)? else {
                    return Ok(None);
                };
                let cached: CachedFloor = serde_json::from_value(value)?;
                if cached.resource != *resource || cached.query != *query {
                    return Err(Error::State("comment floor cache identity mismatch"));
                }
                if tx.get(owner, cache::REVISIONS, &resource.thread()?)? != cached.revision {
                    return Ok(None);
                }
                Ok(Some(cached.page))
            })
        })
    }
}
