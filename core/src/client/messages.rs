mod deletion;
mod pc_poll;
mod projection;
mod reply;

use super::{MusicClient, dto::AccountData};
use crate::account::{AccountSession, SessionToken};
use crate::{Error, Result};
pub(super) use deletion::DeletionHost;
pub use deletion::{MessageDeleteAction, MessageDeleteOutcome, MessagePendingDeletion};
pub(super) use pc_poll::PcMessagePollHost;
pub use pc_poll::{
    MessageCountNavigation, MessageReadAllAction, MessageReadAllState, PcMessageConnectivity,
    PcMessageConnectivityView,
};
pub use projection::attachment::MessageTarget;
use projection::project;
pub(super) use reply::ReplyHost;
pub use reply::{MessageReplyAction, MessageReplyView};
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageCounts {
    pub private: Option<u64>,
    pub comments: Option<u64>,
    pub forwards: Option<u64>,
    pub notices: Option<u64>,
}

struct ObservedCounts {
    counts: MessageCounts,
    hide_private_row_badges: bool,
}

async fn read_counts(session: &AccountSession, token: &SessionToken) -> Result<ObservedCounts> {
    session
        .run_for(token, |api, _| async move {
            let response = api
                .call(
                    crate::protocol::api::MESSAGE_COUNT_PATH,
                    json!({}),
                    false,
                    &[200],
                )
                .await?;
            Ok(ObservedCounts {
                counts: MessageCounts {
                    private: projection::number(&response["msg"]),
                    comments: projection::number(&response["comment"]),
                    forwards: projection::number(&response["forward"]),
                    notices: projection::number(&response["notice"]),
                },
                // M/3090 uses a numeric strict equality for the row display;
                // the general counter projection also accepts decimal strings.
                hide_private_row_badges: response["msg"].as_f64() == Some(0.0),
            })
        })
        .await
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
    pub target: Option<MessageTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deletion_receipt: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub deletion_pending: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    pub rows: Vec<MessageRow>,
    pub next_cursor: Option<u64>,
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_receipt: Option<String>,
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
        let value = read_counts(&self.session, &token).await?.counts;
        Ok(AccountData {
            epoch: token.generation.to_string(),
            value,
        })
    }

    /// Explicit bounded reader; never clears, sends, or marks a message read.
    pub async fn message_page(
        &self,
        epoch: &str,
        query: MessageQuery,
    ) -> Result<AccountData<MessagePage>> {
        let token = self.player_token(epoch)?;
        let revision = self.message_deletion.revision()?;
        let peer = (query.tab == MessageTab::Thread)
            .then(|| query.peer_id.clone())
            .flatten();
        let (mut value, observed) = self
            .session
            .run_for(&token, |api, uid| async move {
                let (path, args, limit) = endpoint(&query, &uid)?;
                let response = api.call(&path, args, false, &[200]).await?;
                let page = project(&query, &response, &uid, limit)?;
                let observed = deletion::observed(&query, &response, &uid);
                Ok((page, observed))
            })
            .await?;
        if let Ok(store) = self.session.persistent_store()
            && (!observed.is_empty() || peer.is_some())
        {
            self.session.with_token(&token, |uid| {
                // Mutations fail closed independently of the original inbox.
                if !observed.is_empty() {
                    let _ = self
                        .message_deletion
                        .publish(&store, &token, uid, revision, &mut value, observed);
                }
                if let Some(peer) = peer
                    && self
                        .message_deletion
                        .observed_peer(&token, &peer, Some(revision))?
                {
                    value.reply_receipt =
                        Some(self.message_reply.observe(&token, &peer, &value.rows)?);
                }
                Ok(())
            })?;
        }
        Ok(AccountData {
            epoch: token.generation.to_string(),
            value,
        })
    }
}

#[cfg(test)]
mod tests;
