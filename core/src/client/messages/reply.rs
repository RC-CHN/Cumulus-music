//! One text send from an observed thread. Receipt recovery uses local metadata
//! only: the server has no proven client request ID or delivery lookup.
use super::{MessageRow, MessageTab, MusicClient, deletion, projection};
use crate::{
    Error, Result,
    account::{SessionToken, profile::AccountProfile},
    client::dto::AccountData,
    storage::StateStore,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};
use uuid::Uuid;

const SEND: &str = "/api/msg/private/send";
const JOURNAL: &str = "private-message-reply-v1";
const MAX_PEERS: usize = 512;
const MAX_SEEN: usize = 128;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MessageReplyAction {
    Status { receipt: String },
    Send { receipt: String, text: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageReplyView {
    receipt: String,
    #[serde(flatten)]
    status: ReplyStatus,
}

#[derive(Serialize)]
#[serde(
    tag = "status",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum ReplyStatus {
    Ready,
    PhoneRequired,
    PreflightUnavailable,
    Unknown {
        operation: String,
    },
    Denied {
        operation: String,
        intercepted: bool,
    },
    Delivered {
        operation: String,
        message_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        row: Option<Box<MessageRow>>,
    },
}

#[derive(Deserialize, Serialize)]
struct Record {
    operation: String,
    phase: Phase,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum Phase {
    Unknown,
    Denied { intercepted: bool },
    Delivered { message_id: String },
}

impl Record {
    fn view(self, row: Option<MessageRow>) -> ReplyStatus {
        let operation = self.operation;
        match self.phase {
            Phase::Unknown => ReplyStatus::Unknown { operation },
            Phase::Denied { intercepted } => ReplyStatus::Denied {
                operation,
                intercepted,
            },
            Phase::Delivered { message_id } => ReplyStatus::Delivered {
                operation,
                message_id,
                row: row.map(Box::new),
            },
        }
    }
}

fn record(store: &StateStore, uid: &str, peer: &str) -> Result<Option<Record>> {
    let record = store
        .get(uid, JOURNAL, peer)?
        .map(serde_json::from_value::<Record>)
        .transpose()?;
    if let Some(record) = &record {
        receipt_id(&record.operation)?;
        if let Phase::Delivered { message_id } = &record.phase
            && projection::positive_id(&json!(message_id)).as_ref() != Some(message_id)
        {
            return Err(Error::State("invalid private reply record"));
        }
    }
    Ok(record)
}

fn receipt_id(value: &str) -> Result<()> {
    if value.len() != 32
        || !value
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err(Error::InvalidInput("invalid private reply receipt"));
    }
    Ok(())
}

fn new_receipt() -> String {
    Uuid::new_v4().simple().to_string()
}

#[derive(Default)]
pub(in crate::client) struct ReplyHost(Mutex<Observations>);

#[derive(Default)]
struct Observations {
    token: Option<SessionToken>,
    peers: VecDeque<Peer>,
}

struct Peer {
    id: String,
    current: String,
    previous: Option<String>,
    seen: VecDeque<String>,
}

impl Peer {
    fn remember(&mut self, id: &str) {
        if !self.seen.iter().any(|existing| existing == id) {
            self.seen.push_back(id.into());
            if self.seen.len() > MAX_SEEN {
                self.seen.pop_front();
            }
        }
    }
}

impl ReplyHost {
    pub(super) fn observe(
        &self,
        token: &SessionToken,
        id: &str,
        rows: &[MessageRow],
    ) -> Result<String> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.token.as_ref() != Some(token) {
            state.token = Some(token.clone());
            state.peers.clear();
        }
        let mut peer = state
            .peers
            .iter()
            .position(|peer| peer.id == id)
            .and_then(|index| state.peers.remove(index))
            .unwrap_or_else(|| Peer {
                id: id.into(),
                current: new_receipt(),
                previous: None,
                seen: VecDeque::new(),
            });
        for row in rows {
            if projection::positive_id(&json!(row.id)).is_some() {
                peer.remember(&row.id);
            }
        }
        let current = peer.current.clone();
        state.peers.push_back(peer);
        if state.peers.len() > MAX_PEERS {
            state.peers.pop_front();
        }
        Ok(current)
    }

    pub(super) fn forget_peer(&self, token: &SessionToken, id: &str) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.token.as_ref() == Some(token) {
            state.peers.retain(|peer| peer.id != id);
        }
        Ok(())
    }

    fn require(
        &self,
        token: &SessionToken,
        receipt: &str,
        write: bool,
    ) -> Result<(String, String)> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.token.as_ref() == Some(token)
            && let Some(peer) = state.peers.iter().find(|peer| {
                peer.current == receipt || (!write && peer.previous.as_deref() == Some(receipt))
            })
        {
            return Ok((peer.id.clone(), peer.current.clone()));
        }
        Err(Error::StaleResponse)
    }

    fn consume(&self, token: &SessionToken, receipt: &str, id: Option<&str>) -> Result<String> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.token.as_ref() == Some(token)
            && let Some(peer) = state.peers.iter_mut().find(|peer| peer.current == receipt)
        {
            let next = new_receipt();
            peer.previous = Some(std::mem::replace(&mut peer.current, next.clone()));
            if let Some(id) = id {
                peer.remember(id);
            }
            return Ok(next);
        }
        Err(Error::StaleResponse)
    }

    fn known_ids(&self, token: &SessionToken, receipt: &str) -> Result<VecDeque<String>> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let peer = state
            .peers
            .iter()
            .find(|peer| peer.current == receipt)
            .filter(|_| state.token.as_ref() == Some(token))
            .ok_or(Error::StaleResponse)?;
        Ok(peer.seen.clone())
    }
}

fn valid_text(text: &str) -> bool {
    // TextArea's default maxCount and JS string.length; JS /^\s*$/ for blank.
    text.encode_utf16().count() <= 1_000
        && text.chars().any(|c| {
            !matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}'
            | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}'
            | '\u{205f}' | '\u{3000}' | '\u{feff}')
        })
        && !text.contains('\0')
}

fn accepted(response: &Value, owner: &str, peer: &str) -> Option<(String, MessageRow)> {
    let [raw] = response["newMsgs"].as_array()?.as_slice() else {
        return None;
    };
    let id = projection::positive_id(&raw["id"])?;
    if projection::user_id(&raw["fromUser"]).as_deref() != Some(owner)
        || raw.get("toUser").is_some_and(|value| {
            !value.is_null() && projection::user_id(value).as_deref() != Some(peer)
        })
        || raw.get("toUserId").is_some_and(|value| {
            !value.is_null() && projection::positive_id(value).as_deref() != Some(peer)
        })
        || !projection::is_plain_reply(&raw["msg"])
    {
        return None;
    }
    Some((id, projection::project_row(MessageTab::Thread, raw, owner)))
}

impl MusicClient {
    pub async fn message_reply(
        &self,
        epoch: &str,
        action: MessageReplyAction,
    ) -> Result<AccountData<MessageReplyView>> {
        let token = self.player_token(epoch)?;
        let store = self.session.persistent_store()?;
        let (operation, text) = match action {
            MessageReplyAction::Status { receipt } => (receipt, None),
            MessageReplyAction::Send { receipt, text } => {
                if !valid_text(&text) {
                    return Err(Error::InvalidInput("invalid private reply text"));
                }
                (receipt, Some(text))
            }
        };
        receipt_id(&operation)?;
        let (uid, peer) = self.session.with_token(&token, |uid| {
            let (peer, _) = self.message_reply.require(&token, &operation, false)?;
            if !self.message_deletion.observed_peer(&token, &peer, None)? {
                return Err(Error::StaleResponse);
            }
            Ok((uid.to_owned(), peer))
        })?;
        let result = |receipt, status| AccountData {
            epoch: epoch.into(),
            value: MessageReplyView { receipt, status },
        };
        let observe_record = |uid: &str| {
            let (observed, current) = self.message_reply.require(&token, &operation, false)?;
            if observed != peer || !self.message_deletion.observed_peer(&token, &peer, None)? {
                return Err(Error::StaleResponse);
            }
            Ok((current, record(&store, uid, &peer)?))
        };
        let Some(text) = text else {
            // Receipt rotation and journal settlement use this same native
            // account lock. A recovered receipt always carries its current
            // successor, even if the request completed as Status began.
            let (current, latest) = self.session.with_token(&token, observe_record)?;
            let status = latest.map_or(ReplyStatus::Ready, |record| record.view(None));
            return Ok(result(current, status));
        };
        let _lock = deletion::lock_peer(&store, &uid, &peer)?;
        let (current, previous) = self.session.with_token(&token, |uid| {
            if deletion::conversation_unresolved(&store, uid, &peer)? {
                return Err(Error::MutationUncertain);
            }
            observe_record(uid)
        })?;
        if let Some(record) = previous
            && (record.operation == operation || matches!(record.phase, Phase::Unknown))
        {
            return Ok(result(current, record.view(None)));
        }
        self.message_reply.require(&token, &operation, true)?;
        let api = self.session.native_api(&token)?;
        let known_binding = self
            .session
            .with_verified_profile(&token, &api, |_, profile| {
                Ok(profile["bindings"]
                    .as_array()
                    .filter(|rows| !rows.is_empty() && rows.len() <= 32)
                    .map(|rows| rows.iter().any(|row| row["type"].as_u64() == Some(1))))
            })?;
        if known_binding == Some(false) {
            return Ok(result(current, ReplyStatus::PhoneRequired));
        }
        if known_binding.is_none() {
            let bindings = AccountProfile::new(self.session.clone())
                .bindings_for(&token)
                .await;
            self.session.check_generation(&token)?;
            let Ok(bindings) = bindings else {
                return Ok(result(current, ReplyStatus::PreflightUnavailable));
            };
            if !bindings.iter().any(|row| row.type_code == 1) {
                return Ok(result(current, ReplyStatus::PhoneRequired));
            }
        }
        let mut write = Record {
            operation,
            phase: Phase::Unknown,
        };
        self.session.require_api(&token, &api)?;
        let known_ids = self.session.with_token(&token, |uid| {
            let known = self.message_reply.known_ids(&token, &write.operation)?;
            if !self.message_deletion.observed_peer(&token, &peer, None)? {
                return Err(Error::StaleResponse);
            }
            store.put(uid, JOURNAL, &peer, &serde_json::to_value(&write)?)?;
            Ok(known)
        })?;
        let payload = json!({"type":"text","id":0,"userIds":[peer],"msg":text});
        let reply = self
            .session
            .run_for(&token, |current, _| {
                let verified = if std::sync::Arc::ptr_eq(&api, &current) {
                    self.session.require_api(&token, &api)
                } else {
                    Err(Error::StaleResponse)
                };
                async move {
                    verified.map_err(|error| Error::RequestNotSent(Box::new(error)))?;
                    current.call(SEND, payload, false, &[200]).await
                }
            })
            .await;
        self.session.with_token(&token, |uid| {
            let row = match reply {
                Ok(response) => {
                    if let Some((id, row)) = accepted(&response, uid, &peer)
                        && !known_ids.contains(&id)
                    {
                        write.phase = Phase::Delivered { message_id: id };
                        Some(row)
                    } else {
                        None
                    }
                }
                Err(error) => {
                    let intercepted = error.is_service_code(2202);
                    if intercepted || matches!(error.root(), Error::RequestNotSent(_)) {
                        write.phase = Phase::Denied { intercepted };
                    }
                    None
                }
            };
            if matches!(write.phase, Phase::Unknown) {
                return Ok(result(current, write.view(None)));
            }
            store.put(uid, JOURNAL, &peer, &serde_json::to_value(&write)?)?;
            let id = match &write.phase {
                Phase::Delivered { message_id } => Some(message_id.as_str()),
                _ => None,
            };
            let next = self.message_reply.consume(&token, &write.operation, id)?;
            Ok(result(next, write.view(row)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acknowledgement_requires_one_real_own_text_row_for_the_requested_peer() {
        let valid = json!({"id":"9007199254741997","fromUser":{"userId":2},
            "msg":{"type":6,"msg":"synthetic response"}});
        let reply = |rows| json!({"newMsgs":rows});
        assert!(accepted(&reply(vec![valid.clone()]), "2", "505").is_some());
        let mut wrong = valid.clone();
        wrong["toUserId"] = json!("506");
        assert!(accepted(&reply(vec![wrong]), "2", "505").is_none());
        let mut wrong = valid.clone();
        wrong["fromUser"]["userId"] = json!(505);
        assert!(accepted(&reply(vec![wrong]), "2", "505").is_none());
        let mut wrong = valid.clone();
        wrong["id"] = json!(0);
        assert!(accepted(&reply(vec![wrong]), "2", "505").is_none());
        assert!(accepted(&reply(vec![valid.clone(), valid]), "2", "505").is_none());
    }
}
