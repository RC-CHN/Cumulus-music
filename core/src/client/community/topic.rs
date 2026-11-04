//! M/434, M/359, M/3006 and M/3105: read-only per-hashtag activities.
use super::{CardLease, CommunityCard, CommunityHost, MusicClient, handle, project};
use crate::{
    Error, Result,
    account::SessionToken,
    client::{dto::AccountData, mapping},
    error::code_matches,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashSet, VecDeque},
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const MAX_VISITS: usize = 3;
const MAX_ROWS: usize = 20;
const MAX_ALL: usize = 140;
const MAX_CURSORS: usize = 1000;
const ALL: u8 = 1;
const HOT: u8 = 2;
const READ_CODES: &[i64] = &[200, 403, 404];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityTopic {
    handle: String,
    title: String,
    description: Option<String>,
    cover_url: Option<String>,
    wide_cover_url: Option<String>,
    participants: Option<u64>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommunityTopicAction {
    Hot,
    First,
    Next { continuation: String },
    Refresh,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityTopicSlice {
    cards: Vec<CommunityCard>,
    continuation: Option<String>,
    replace: bool,
    unavailable: bool,
    retired: Vec<String>,
}

#[derive(Default)]
struct TopicState {
    generation: Option<u64>,
    opening: Option<Uuid>,
    visits: VecDeque<Visit>,
}
struct Visit {
    key: Uuid,
    act: Value,
    all_pending: Option<Uuid>,
    hot_pending: Option<Uuid>,
    continuation: Option<(Uuid, Value)>,
    cursors: HashSet<String>,
    all_order: VecDeque<Uuid>,
}
#[derive(Default)]
pub(in crate::client) struct TopicHost {
    state: Mutex<TopicState>,
    pub(super) cards: CommunityHost,
}
impl TopicHost {
    pub(super) fn retire_deleted(&self, token: &SessionToken, retired: &[Uuid]) -> Result<()> {
        if !retired.is_empty() {
            for visit in &mut self.state(token)?.visits {
                visit.all_order.retain(|key| !retired.contains(key));
            }
        }
        Ok(())
    }

    fn state(&self, token: &SessionToken) -> Result<MutexGuard<'_, TopicState>> {
        let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        if state.generation != Some(token.generation) {
            *state = TopicState {
                generation: Some(token.generation),
                ..Default::default()
            };
        }
        Ok(state)
    }
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.chars().count() <= 80
        && !name
            .chars()
            .any(|c| matches!(c, '[' | ']' | '/' | '\\' | '#') || c.is_control())
}

fn unavailable(error: &Error) -> bool {
    error.is_service_code(403)
        || error.is_service_code(404)
        || matches!(
            error.root(),
            Error::HttpStatus {
                status: 403 | 404,
                ..
            }
        )
}

fn topic_meta(name: &str, key: Uuid, raw: &Value) -> CommunityTopic {
    let description = match &raw["text"] {
        Value::Array(values) => values.first().and_then(|v| project::text(v, 8000)),
        Value::Null => None,
        value => project::text(&json!(value.to_string()), 8000),
    };
    CommunityTopic {
        handle: key.to_string(),
        title: format!("#{name}#"),
        description,
        cover_url: mapping::artwork(&raw["coverPCUrl"]),
        wide_cover_url: mapping::artwork(&raw["coverPCLongUrl"]),
        participants: raw["participateCount"]
            .as_u64()
            .filter(|n| *n <= 9_007_199_254_740_991),
    }
}

impl MusicClient {
    pub async fn community_topic(
        &self,
        epoch: &str,
        name: &str,
    ) -> Result<AccountData<Option<CommunityTopic>>> {
        let token = self.playlist_token(epoch)?;
        if !valid_name(name) {
            return Err(Error::InvalidInput("invalid community topic name"));
        }
        let host = &self.community_topics;
        let key = Uuid::new_v4();
        self.session.with_token(&token, |_| {
            host.state(&token)?.opening = Some(key);
            Ok(())
        })?;
        let response = self
            .session
            .run_for(&token, |api, _| async move {
                match api
                    .call(
                        "/api/act/detail",
                        json!({"actname":name}),
                        false,
                        READ_CODES,
                    )
                    .await
                {
                    Ok(reply) if code_matches(&reply["code"], 200) => Ok(Some(reply)),
                    Ok(_) => Ok(None),
                    Err(error) if unavailable(&error) => Ok(None),
                    Err(error) => Err(error),
                }
            })
            .await;
        self.session.with_token(&token, |_| {
            let mut state = host.state(&token)?;
            if state.opening != Some(key) {
                return Err(Error::StaleResponse);
            }
            state.opening = None;
            let value = if let Some(reply) = response? {
                let raw = &reply["act"];
                project::id(&raw["actId"])
                    .ok_or(Error::Response("invalid community topic activity"))?;
                let mut card_state = host.cards.state(&token)?;
                if state.visits.len() >= MAX_VISITS
                    && let Some(old) = state.visits.pop_front()
                {
                    card_state
                        .cards
                        .retain(|r| r.topic.is_none_or(|(key, _)| key != old.key));
                }
                state.visits.push_back(Visit {
                    key,
                    act: raw["actId"].clone(),
                    all_pending: None,
                    hot_pending: None,
                    continuation: None,
                    cursors: HashSet::new(),
                    all_order: VecDeque::new(),
                });
                Some(topic_meta(name, key, raw))
            } else {
                None
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub async fn community_topic_notice(
        &self,
        epoch: &str,
        topic: &str,
    ) -> Result<AccountData<bool>> {
        let token = self.playlist_token(epoch)?;
        let topic = handle(topic)?;
        let host = &self.community_topics;
        let act = self.session.with_token(&token, |_| {
            host.state(&token)?
                .visits
                .iter()
                .find(|visit| visit.key == topic)
                .map(|visit| visit.act.clone())
                .ok_or(Error::StaleResponse)
        })?;
        let now = u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Error::State("community topic clock unavailable"))?
                .as_millis(),
        )
        .map_err(|_| Error::State("community topic clock unavailable"))?;
        let body = json!({"actid":act,"lasttime":now});
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                let reply = api.call("/api/act/event/new", body, false, &[200]).await?;
                reply["new"]
                    .as_bool()
                    .ok_or(Error::Response("invalid community topic notice"))
            })
            .await?;
        self.session.with_token(&token, |_| {
            if !host
                .state(&token)?
                .visits
                .iter()
                .any(|visit| visit.key == topic)
            {
                return Err(Error::StaleResponse);
            }
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub async fn community_topic_slice(
        &self,
        epoch: &str,
        topic: &str,
        action: CommunityTopicAction,
    ) -> Result<AccountData<CommunityTopicSlice>> {
        let token = self.playlist_token(epoch)?;
        let host = &self.community_topics;
        let topic = handle(topic)?;
        let request = self
            .session
            .with_token(&token, |_| host.begin(&token, topic, action))?;
        let (path, body) = if let Some(cursor) = &request.cursor {
            (
                "/api/act/event",
                json!({"actid":request.act,"lasttime":cursor}),
            )
        } else {
            ("/api/act/event/hot", json!({"actid":request.act}))
        };
        let result = self
            .session
            .run_for(&token, |api, _| async move {
                match api.call(path, body, false, READ_CODES).await {
                    Ok(reply) if code_matches(&reply["code"], 200) => Ok(Some(reply)),
                    Ok(_) => Ok(None),
                    Err(error) if unavailable(&error) => Ok(None),
                    Err(error) => Err(error),
                }
            })
            .await?;
        self.session.with_token(&token, |owner| {
            let value = request.settle(self, &token, owner, result.as_ref())?;
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}

struct SliceLease<'a> {
    host: &'a TopicHost,
    generation: u64,
    topic: Uuid,
    request: Uuid,
    act: Value,
    cursor: Option<Value>,
    replace: bool,
}
impl Visit {
    fn pending(&mut self, hot: bool) -> &mut Option<Uuid> {
        if hot {
            &mut self.hot_pending
        } else {
            &mut self.all_pending
        }
    }
}
impl TopicHost {
    fn begin(
        &self,
        token: &SessionToken,
        topic: Uuid,
        action: CommunityTopicAction,
    ) -> Result<SliceLease<'_>> {
        let mut state = self.state(token)?;
        let visit = state
            .visits
            .iter_mut()
            .find(|visit| visit.key == topic)
            .ok_or(Error::StaleResponse)?;
        let (cursor, replace) = match action {
            CommunityTopicAction::Hot => (None, true),
            CommunityTopicAction::First => (Some(json!("-1")), true),
            CommunityTopicAction::Refresh => (Some(json!(-1)), true),
            CommunityTopicAction::Next { continuation } => {
                let key = handle(&continuation)?;
                let (_, cursor) = visit
                    .continuation
                    .as_ref()
                    .filter(|(lease, _)| *lease == key)
                    .ok_or(Error::StaleResponse)?;
                (Some(cursor.clone()), false)
            }
        };
        let request = Uuid::new_v4();
        let slot = visit.pending(cursor.is_none());
        if slot.is_some() {
            return Err(Error::State("community topic read pending"));
        }
        *slot = Some(request);
        Ok(SliceLease {
            host: self,
            generation: token.generation,
            topic,
            request,
            act: visit.act.clone(),
            cursor,
            replace,
        })
    }
}
impl Drop for SliceLease<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.host.state.lock()
            && state.generation == Some(self.generation)
            && let Some(visit) = state
                .visits
                .iter_mut()
                .find(|visit| visit.key == self.topic)
        {
            let slot = visit.pending(self.cursor.is_none());
            if *slot == Some(self.request) {
                *slot = None;
            }
        }
    }
}
impl SliceLease<'_> {
    fn settle(
        &self,
        client: &MusicClient,
        token: &SessionToken,
        owner: &str,
        reply: Option<&Value>,
    ) -> Result<CommunityTopicSlice> {
        let mut state = self.host.state(token)?;
        let index = state
            .visits
            .iter()
            .position(|visit| visit.key == self.topic)
            .ok_or(Error::StaleResponse)?;
        let visit = &mut state.visits[index];
        let hot = self.cursor.is_none();
        if *visit.pending(hot) != Some(self.request) {
            return Err(Error::StaleResponse);
        }
        let mut cards = self.host.cards.state(token)?;
        let Some(reply) = reply else {
            state.visits.remove(index);
            cards
                .cards
                .retain(|record| record.topic.is_none_or(|(key, _)| key != self.topic));
            return Ok(CommunityTopicSlice {
                cards: Vec::new(),
                continuation: None,
                replace: true,
                unavailable: true,
                retired: Vec::new(),
            });
        };
        let raw = reply["events"]
            .as_array()
            .filter(|events| events.len() <= MAX_ROWS)
            .ok_or(Error::Response("invalid community topic rows"))?;
        let next = if !hot
            && reply["more"]
                .as_bool()
                .ok_or(Error::Response("invalid community topic more"))?
        {
            let cursor = project::cursor(&reply["lasttime"])
                .ok_or(Error::Response("invalid community topic cursor"))?;
            let key = project::cursor_key(&cursor).expect("validated cursor");
            if !self.replace && (visit.cursors.contains(&key) || visit.cursors.len() >= MAX_CURSORS)
            {
                return Err(Error::Response("community topic cursor did not advance"));
            }
            Some((cursor, key))
        } else {
            None
        };
        let group = if hot { HOT } else { ALL };
        if self.replace {
            cards.cards.retain_mut(|record| {
                let Some((topic, membership)) = &mut record.topic else {
                    return true;
                };
                if *topic != self.topic {
                    return true;
                }
                *membership &= !group;
                *membership != 0
            });
            if !hot {
                visit.all_order.clear();
                visit.cursors.clear();
            }
        }
        let mut projected: Vec<CommunityCard> = Vec::new();
        for (event, locator, mut card) in client
            .note_visible_rows(owner, raw)?
            .into_iter()
            .filter(|row| {
                row["actId"].is_null() || project::id(&row["actId"]) == project::id(&self.act)
            })
            .filter_map(|row| project::card(row, true))
        {
            let record =
                match cards.cards.iter_mut().find(|r| {
                    r.event == event && r.topic.is_some_and(|(topic, _)| topic == self.topic)
                }) {
                    Some(record) => {
                        if record.locator != locator {
                            card.author_name = "匿名用户".into();
                            card.avatar_url = None;
                            record.locator = None;
                            record.detail_read = None;
                            record.comments_receipt = None;
                        }
                        if let Some((_, membership)) = &mut record.topic {
                            *membership |= group;
                        }
                        record
                    }
                    None => {
                        cards.cards.push_back(CardLease {
                            key: Uuid::new_v4(),
                            event,
                            locator,
                            detail_read: None,
                            comments_receipt: None,
                            topic: Some((self.topic, group)),
                        });
                        cards.cards.back_mut().expect("added topic card")
                    }
                };
            if !hot && !visit.all_order.contains(&record.key) {
                visit.all_order.push_back(record.key);
            }
            card.key = record.key.to_string();
            if record.locator.is_some() {
                card.detail = Some(card.key.clone());
                card.can_delete = record
                    .locator
                    .as_ref()
                    .is_some_and(|locator| locator.owner == owner);
            }
            if let Some(existing) = projected
                .iter_mut()
                .find(|existing| existing.key == card.key)
            {
                *existing = card;
            } else {
                projected.push(card);
            }
        }
        let mut retired = Vec::new();
        if !hot {
            while visit.all_order.len() > MAX_ALL {
                let key = visit.all_order.pop_front().expect("over capacity");
                cards.cards.retain_mut(|record| {
                    if record.key != key {
                        return true;
                    }
                    if let Some((_, membership)) = &mut record.topic {
                        *membership &= !ALL;
                        *membership != 0
                    } else {
                        true
                    }
                });
                retired.push(key.to_string());
            }
            visit.continuation = next.map(|(cursor, key)| {
                visit.cursors.insert(key);
                (Uuid::new_v4(), cursor)
            });
        }
        Ok(CommunityTopicSlice {
            cards: projected,
            continuation: if hot {
                None
            } else {
                visit.continuation.as_ref().map(|(id, _)| id.to_string())
            },
            replace: self.replace,
            unavailable: false,
            retired,
        })
    }
}
