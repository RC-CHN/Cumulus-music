use crate::{Error, Result, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Playlist,
    Voice,
    Event,
    Album,
    Track,
    Mv,
    Topic,
    Video,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommentResource {
    pub kind: ResourceKind,
    pub id: String,
    #[serde(default)]
    pub owner: Option<String>,
}
impl CommentResource {
    pub fn thread(&self) -> Result<String> {
        if self.id.len() > 128 {
            return Err(Error::InvalidInput("resource identifier is too long"));
        }
        if self.kind == ResourceKind::Video {
            if self.id.is_empty() || !self.id.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return Err(Error::InvalidInput("invalid video identifier"));
            }
        } else {
            identifier(&json!(self.id))?;
        }
        if (self.kind == ResourceKind::Event) != self.owner.is_some() {
            return Err(Error::InvalidInput("only events require a resource owner"));
        }
        let prefix = match self.kind {
            ResourceKind::Playlist => "A_PL_0",
            ResourceKind::Voice => "A_DJ_1",
            ResourceKind::Event => "A_EV_2",
            ResourceKind::Album => "R_AL_3",
            ResourceKind::Track => "R_SO_4",
            ResourceKind::Mv => "R_MV_5",
            ResourceKind::Topic => "A_TO_6",
            ResourceKind::Video => "R_VI_62",
        };
        Ok(if let Some(owner) = &self.owner {
            format!("{prefix}_{}_{}", self.id, identifier(&json!(owner))?)
        } else {
            format!("{prefix}_{}", self.id)
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentSort {
    Latest,
    Hot,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommentQuery {
    pub sort: CommentSort,
    pub limit: usize,
    pub offset: usize,
    pub before_time: u64,
    pub comment_id: Option<String>,
    #[serde(default)]
    pub compare_user_location: bool,
    #[serde(default)]
    pub compose_concert: bool,
    #[serde(default)]
    pub mark_replied: bool,
    #[serde(default)]
    pub force_flat_comment: bool,
    #[serde(default)]
    pub show_inner: bool,
}
impl Default for CommentQuery {
    fn default() -> Self {
        Self {
            sort: CommentSort::Latest,
            limit: 60,
            offset: 0,
            before_time: 0,
            comment_id: None,
            compare_user_location: false,
            compose_concert: false,
            mark_replied: false,
            force_flat_comment: false,
            show_inner: false,
        }
    }
}
impl CommentQuery {
    pub fn validate(&self) -> Result<()> {
        if !(1..=100).contains(&self.limit)
            || self.offset > 1_000_000
            || self.before_time > 9_007_199_254_740_991
        {
            return Err(Error::InvalidInput("invalid comment page bounds"));
        }
        if let Some(id) = &self.comment_id {
            identifier(&json!(id))?;
            if self.sort == CommentSort::Hot {
                return Err(Error::InvalidInput("hot pages cannot locate a comment"));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommentAuthor {
    pub id: String,
    pub nickname: String,
    pub avatar: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommentReply {
    pub id: Option<String>,
    pub content: Option<String>,
    pub author: Option<CommentAuthor>,
    pub status: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub content: String,
    pub time: u64,
    pub time_text: String,
    pub liked: Option<bool>,
    pub liked_count: u64,
    pub author: Option<CommentAuthor>,
    pub replies: Vec<CommentReply>,
    pub status: Option<i64>,
    // Retain recovered presentation data natively; IPC has its own whitelist.
    pub rich_content: Value,
    pub show_floor_comment: Value,
    pub decoration: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommentPage {
    pub comments: Vec<Comment>,
    pub hot_comments: Vec<Comment>,
    pub top_comments: Vec<Comment>,
    pub current_comment: Option<Comment>,
    pub total: u64,
    pub more: bool,
    pub more_hot: bool,
    pub next: Option<CommentQuery>,
}
fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().into()
}
pub(super) fn number(v: &Value) -> Result<u64> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .ok_or(Error::Response("invalid comment number"))
}
fn author(v: &Value) -> Result<Option<CommentAuthor>> {
    if v.is_null() {
        return Ok(None);
    }
    Ok(Some(CommentAuthor {
        id: identifier(&v["userId"])?,
        nickname: text(&v["nickname"]),
        avatar: v["avatarUrl"].as_str().map(str::to_owned),
    }))
}
impl Comment {
    pub fn from_official(v: &Value) -> Result<Self> {
        let replies = match v.get("beReplied").filter(|v| !v.is_null()) {
            Some(Value::Array(rows)) => rows
                .iter()
                .map(|r| {
                    Ok(CommentReply {
                        id: r
                            .get("beRepliedCommentId")
                            .filter(|v| !v.is_null() && **v != 0)
                            .map(identifier)
                            .transpose()?,
                        content: r["content"].as_str().map(str::to_owned),
                        author: author(&r["user"])?,
                        status: r["status"].as_i64(),
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            None => vec![],
            _ => return Err(Error::Response("invalid comment replies")),
        };
        let liked = match &v["liked"] {
            Value::Null => None,
            Value::Bool(v) => Some(*v),
            _ => return Err(Error::Response("invalid comment liked flag")),
        };
        Ok(Self {
            id: identifier(&v["commentId"])?,
            parent_id: v
                .get("parentCommentId")
                .filter(|v| !v.is_null() && **v != 0 && **v != "0")
                .map(identifier)
                .transpose()?,
            content: text(&v["content"]),
            time: number(&v["time"])?,
            time_text: text(&v["timeStr"]),
            liked,
            liked_count: number(&v["likedCount"])?,
            author: author(&v["user"])?,
            replies,
            status: v["status"].as_i64(),
            rich_content: v["richContent"].clone(),
            show_floor_comment: v["showFloorComment"].clone(),
            decoration: v["decoration"].clone(),
        })
    }
}
pub(super) fn rows(value: &Value, required: bool) -> Result<Vec<Comment>> {
    if value.is_null() && !required {
        return Ok(vec![]);
    }
    let rows = value
        .as_array()
        .ok_or(Error::Response("missing comments array"))?;
    if rows.len() > 1000 {
        return Err(Error::Response("comment page is too large"));
    }
    let mut ids = std::collections::BTreeSet::new();
    rows.iter()
        .map(|v| {
            let c = Comment::from_official(v)?;
            if !ids.insert(c.id.clone()) {
                return Err(Error::Response("comment page repeats an identifier"));
            }
            Ok(c)
        })
        .collect()
}
