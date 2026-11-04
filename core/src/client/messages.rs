mod projection;

use super::{MusicClient, dto::AccountData};
use crate::{Error, Result};
use projection::project;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageTab {
    Private,
    Comments,
    Forwards,
    Notices,
    Thread,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageQuery {
    pub tab: MessageTab,
    pub cursor: Option<u64>,
    pub peer_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageCounts {
    pub private: Option<u64>,
    pub comments: Option<u64>,
    pub forwards: Option<u64>,
    pub notices: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRow {
    pub id: String,
    pub peer_id: Option<String>,
    pub author: String,
    pub body: String,
    pub caption: Option<String>,
    pub quote: Option<String>,
    pub time: Option<u64>,
    pub unread: Option<u64>,
    pub own: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    pub rows: Vec<MessageRow>,
    pub next_cursor: Option<u64>,
    pub hint: Option<String>,
}

const MAX_SAFE_TIME: u64 = 9_007_199_254_740_991;
const fn valid_cursor(value: u64) -> bool {
    value > 0 && value <= MAX_SAFE_TIME
}

fn endpoint(query: &MessageQuery, uid: &str) -> Result<(String, Value, usize)> {
    let time = match query.cursor {
        Some(value) if valid_cursor(value) => json!(value),
        None => json!(-1),
        _ => return Err(Error::InvalidInput("invalid message cursor")),
    };
    if let Some(peer) = query.peer_id.as_deref()
        && (query.tab != MessageTab::Thread || projection::positive_id(&json!(peer)).is_none())
    {
        return Err(Error::InvalidInput("invalid message peer"));
    }
    let (path, args, limit) = match query.tab {
        MessageTab::Private => (
            "/api/msg/private/users".into(),
            json!({"limit":15,"offset":0,"time":time}),
            15,
        ),
        MessageTab::Comments => (
            format!("/api/v1/user/comments/{uid}"),
            json!({"limit":10,"beforeTime":time}),
            10,
        ),
        MessageTab::Forwards => (
            "/api/forwards/get".into(),
            json!({"total":true,"limit":10,"offset":0,"time":time}),
            10,
        ),
        MessageTab::Notices => (
            "/api/msg/notices".into(),
            json!({"total":true,"limit":10,"offset":0,"time":time}),
            10,
        ),
        MessageTab::Thread => (
            "/api/msg/private/history".into(),
            json!({"limit":10,"offset":0,"time":time,"userId":query.peer_id.as_deref()
                .ok_or(Error::InvalidInput("missing message peer"))?}),
            10,
        ),
    };
    Ok((path, args, limit))
}

impl MusicClient {
    pub async fn message_counts(&self, epoch: &str) -> Result<AccountData<MessageCounts>> {
        let token = self.player_token(epoch)?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                let response = api.call("/api/pl/count", json!({}), false, &[200]).await?;
                Ok(MessageCounts {
                    private: projection::number(&response["msg"]),
                    comments: projection::number(&response["comment"]),
                    forwards: projection::number(&response["forward"]),
                    notices: projection::number(&response["notice"]),
                })
            })
            .await?;
        Ok(AccountData {
            epoch: token.generation.to_string(),
            value,
        })
    }

    /// No clear/send/delete operation or periodic reader is registered.
    pub async fn message_page(
        &self,
        epoch: &str,
        query: MessageQuery,
    ) -> Result<AccountData<MessagePage>> {
        let token = self.player_token(epoch)?;
        let value = self
            .session
            .run_for(&token, |api, uid| async move {
                let (path, args, limit) = endpoint(&query, &uid)?;
                let response = api.call(&path, args, false, &[200]).await?;
                project(&query, &response, &uid, limit)
            })
            .await?;
        Ok(AccountData {
            epoch: token.generation.to_string(),
            value,
        })
    }
}

#[cfg(test)]
mod tests;
