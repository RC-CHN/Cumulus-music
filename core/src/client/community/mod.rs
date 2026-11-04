pub mod composer;
pub mod new_song_notice;
mod project;

use super::{MusicClient, comments, dto::AccountData};
use crate::{
    Error, Result,
    account::SessionToken,
    comments::{Comment, CommentApi, CommentQuery, CommentResource, ResourceKind},
    protocol::api::validate_response,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashSet, VecDeque},
    sync::Mutex,
};
use uuid::Uuid;

const FEED: &str = "/api/event/pc/history/feed/get";
const MAX_PAGE_ROWS: usize = 30;
const MAX_FEED_ROWS: usize = 480;
const MAX_CURSORS: usize = 1000;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommunityFeedAction {
    First,
    Refresh,
    Next { continuation: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityFeedPage {
    cards: Vec<CommunityCard>,
    continuation: Option<String>,
    replace: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityCard {
    key: String,
    author_name: String,
    avatar_url: Option<String>,
    event_time: Option<u64>,
    time_text: Option<String>,
    title: Option<String>,
    message: Option<String>,
    pictures: Vec<String>,
    attachment: Option<CommunityAttachment>,
    counters: CommunityCounters,
    status: CommunityCardStatus,
    detail: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum CommunityCardStatus {
    Ready,
    Unsupported,
    Deleted,
    Unavailable,
}

#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum CommunityAttachment {
    Resource {
        kind: &'static str,
        id: String,
        label: String,
        cover_url: Option<String>,
    },
    Forwarded {
        card: Box<CommunityCard>,
    },
    Unavailable {
        label: &'static str,
    },
}

#[derive(Serialize)]
struct CommunityCounters {
    likes: Option<u64>,
    shares: Option<u64>,
    comments: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityDetail {
    card: CommunityCard,
    comments_available: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityCommentPage {
    comments: Vec<CommunityComment>,
    hot_comments: Vec<CommunityComment>,
    next_offset: Option<usize>,
    total: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CommunityComment {
    id: String,
    author_name: Option<String>,
    text: String,
    time: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Locator {
    event: String,
    owner: String,
}
impl Locator {
    fn comments(&self) -> CommentResource {
        CommentResource {
            kind: ResourceKind::Event,
            id: self.event.clone(),
            owner: Some(self.owner.clone()),
        }
    }
}

struct CardLease {
    key: Uuid,
    event: String,
    locator: Option<Locator>,
    comments_receipt: Option<Uuid>,
    detail_read: Option<Uuid>,
}
#[derive(Default)]
struct State {
    generation: Option<u64>,
    pending: Option<Uuid>,
    continuation: Option<(Uuid, Value)>,
    cards: VecDeque<CardLease>,
    visited: HashSet<String>,
    attachments: VecDeque<composer::AttachmentReceipt>,
}
#[derive(Default)]
pub(super) struct CommunityHost(Mutex<State>);

fn handle(text: &str) -> Result<Uuid> {
    if text.len() != 36 {
        return Err(Error::InvalidInput("invalid community read handle"));
    }
    Uuid::parse_str(text).map_err(|_| Error::InvalidInput("invalid community read handle"))
}

impl CommunityHost {
    fn state(&self, token: &SessionToken) -> Result<std::sync::MutexGuard<'_, State>> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.generation != Some(token.generation) {
            *state = State {
                generation: Some(token.generation),
                ..Default::default()
            };
        }
        Ok(state)
    }

    fn begin(&self, token: &SessionToken, action: CommunityFeedAction) -> Result<ReadLease<'_>> {
        let mut state = self.state(token)?;
        if state.pending.is_some() {
            return Err(Error::State("community page read is pending"));
        }
        let (cursor, refresh) = match action {
            CommunityFeedAction::First => (json!("-1"), false),
            CommunityFeedAction::Refresh => (json!(-1), true),
            CommunityFeedAction::Next { continuation } => {
                let key = handle(&continuation)?;
                let (_, cursor) = state
                    .continuation
                    .as_ref()
                    .filter(|(lease, _)| *lease == key)
                    .ok_or(Error::StaleResponse)?;
                (cursor.clone(), false)
            }
        };
        let request = Uuid::new_v4();
        state.pending = Some(request);
        Ok(ReadLease {
            host: self,
            generation: token.generation,
            request,
            cursor,
            refresh,
        })
    }

    fn comments_receipt(&self, token: &SessionToken, key: Uuid) -> Result<(Locator, Uuid)> {
        let state = self.state(token)?;
        state
            .cards
            .iter()
            .find(|record| record.key == key)
            .and_then(|record| Some((record.locator.clone()?, record.comments_receipt?)))
            .ok_or(Error::StaleResponse)
    }
}

struct ReadLease<'a> {
    host: &'a CommunityHost,
    generation: u64,
    request: Uuid,
    cursor: Value,
    refresh: bool,
}
impl Drop for ReadLease<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.host.0.lock()
            && state.generation == Some(self.generation)
            && state.pending == Some(self.request)
        {
            state.pending = None;
        }
    }
}
impl ReadLease<'_> {
    fn settle(&self, token: &SessionToken, response: &Value) -> Result<CommunityFeedPage> {
        let data = &response["data"];
        let raw = data["events"]
            .as_array()
            .filter(|rows| rows.len() <= MAX_PAGE_ROWS)
            .ok_or(Error::Response("invalid community feed rows"))?;
        let more = data["more"]
            .as_bool()
            .ok_or(Error::Response("missing community continuation flag"))?;
        let replace = self.refresh || self.cursor == "-1";
        let next = if !more {
            None
        } else if self.refresh {
            // M3006's refresh reducer deliberately resets cursor to "-1".
            Some(json!("-1"))
        } else {
            let next = project::cursor(&data["cursor"])
                .ok_or(Error::Response("invalid community continuation"))?;
            Some(next)
        };

        let mut state = self.host.state(token)?;
        if state.pending != Some(self.request) {
            return Err(Error::StaleResponse);
        }
        let next_key = next.as_ref().and_then(project::cursor_key);
        if !replace
            && next_key.as_ref().is_some_and(|key| {
                state.visited.contains(key) || state.visited.len() >= MAX_CURSORS
            })
        {
            return Err(Error::Response("community continuation did not advance"));
        }
        if replace {
            state.cards.clear();
            state.visited.clear();
        }
        if let Some(key) = next_key {
            state.visited.insert(key);
        }
        let mut cards: Vec<CommunityCard> = Vec::new();
        for (event, locator, mut card) in raw.iter().filter_map(|row| project::card(row, true)) {
            let record = match state.cards.iter_mut().find(|r| r.event == event) {
                Some(record) => {
                    if record.locator != locator {
                        // The stable row key can update its card, but must
                        // never grant an old handle a different author.
                        record.locator = None;
                        record.detail_read = None;
                        record.comments_receipt = None;
                    }
                    record
                }
                None => {
                    state.cards.push_back(CardLease {
                        key: Uuid::new_v4(),
                        event,
                        locator,
                        comments_receipt: None,
                        detail_read: None,
                    });
                    state.cards.back_mut().expect("added card")
                }
            };
            card.key = record.key.to_string();
            if record.locator.is_some() {
                card.detail = Some(card.key.clone());
            }
            if let Some(previous) = cards.iter_mut().find(|previous| previous.key == card.key) {
                *previous = card;
            } else {
                cards.push(card);
            }
        }
        while state.cards.len() > MAX_FEED_ROWS {
            state.cards.pop_front();
        }
        state.continuation = next.map(|value| (Uuid::new_v4(), value));
        Ok(CommunityFeedPage {
            cards,
            continuation: state.continuation.as_ref().map(|(key, _)| key.to_string()),
            replace,
        })
    }
}

fn comment(value: Comment) -> CommunityComment {
    let view = comments::comment(value, "");
    CommunityComment {
        id: view.id,
        author_name: view.author.map(|author| author.name),
        text: view.content,
        time: view.time,
    }
}

impl MusicClient {
    pub async fn community_feed(
        &self,
        epoch: &str,
        action: CommunityFeedAction,
    ) -> Result<AccountData<CommunityFeedPage>> {
        let token = self.playlist_token(epoch)?;
        let request = self
            .session
            .with_token(&token, |_| self.community.begin(&token, action))?;
        let args = json!({"cursor": request.cursor});
        let response = self
            .session
            .run_for(&token, |api, _| async move {
                api.call(FEED, args, false, &[200]).await
            })
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: request.settle(&token, &response)?,
            })
        })
    }

    pub async fn community_detail(
        &self,
        epoch: &str,
        detail: &str,
    ) -> Result<AccountData<Option<CommunityDetail>>> {
        let token = self.playlist_token(epoch)?;
        let key = handle(detail)?;
        let detail_read = Uuid::new_v4();
        let locator = self.session.with_token(&token, |_| {
            let mut state = self.community.state(&token)?;
            let record = state
                .cards
                .iter_mut()
                .find(|record| record.key == key)
                .ok_or(Error::StaleResponse)?;
            let locator = record.locator.clone().ok_or(Error::StaleResponse)?;
            record.comments_receipt = None;
            record.detail_read = Some(detail_read);
            Ok(locator)
        })?;
        let path = format!("/api/v1/event/{}/{}", locator.owner, locator.event);
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                let reply = validate_response(
                    &path,
                    api.request(&path, json!({}), false).await?,
                    &[200, 403, 404, 410],
                )?;
                if crate::error::code_matches(&reply["code"], 200) {
                    Ok(reply.get("event").cloned().filter(|value| !value.is_null()))
                } else {
                    Ok(None)
                }
            })
            .await?;
        self.session.with_token(&token, |_| {
            let mut state = self.community.state(&token)?;
            let record = state
                .cards
                .iter_mut()
                .find(|record| {
                    record.key == key
                        && record.locator.as_ref() == Some(&locator)
                        && record.detail_read == Some(detail_read)
                })
                .ok_or(Error::StaleResponse)?;
            record.detail_read = None;
            let value = if let Some(raw) = value {
                let (_, owner, mut card) = project::card(&raw, true)
                    .ok_or(Error::Response("invalid community event detail"))?;
                if owner.as_ref() != Some(&locator) {
                    return Err(Error::Response("community event detail identity mismatch"));
                }
                card.key = detail.into();
                let resource = locator.comments();
                let comments_available =
                    raw["info"]["threadId"].as_str() == Some(resource.thread()?.as_str());
                record.comments_receipt = comments_available.then_some(detail_read);
                Some(CommunityDetail {
                    card,
                    comments_available,
                })
            } else {
                None
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub async fn community_comments(
        &self,
        epoch: &str,
        detail: &str,
        offset: usize,
    ) -> Result<AccountData<CommunityCommentPage>> {
        let token = self.playlist_token(epoch)?;
        let key = handle(detail)?;
        let (locator, receipt) = self
            .session
            .with_token(&token, |_| self.community.comments_receipt(&token, key))?;
        let resource = locator.comments();
        let query = CommentQuery {
            offset,
            ..Default::default()
        };
        let page = self
            .session
            .run_for(&token, |api, _| async move {
                CommentApi::new(api).page(&resource, &query).await
            })
            .await?;
        self.session.with_token(&token, |_| {
            if self.community.comments_receipt(&token, key)? != (locator, receipt) {
                return Err(Error::StaleResponse);
            }
            Ok(AccountData {
                epoch: epoch.into(),
                value: CommunityCommentPage {
                    next_offset: page.next.map(|query| query.offset),
                    comments: page.comments.into_iter().map(comment).collect(),
                    hot_comments: if offset == 0 {
                        page.hot_comments
                            .into_iter()
                            .take(20)
                            .map(comment)
                            .collect()
                    } else {
                        Vec::new()
                    },
                    total: page.total,
                },
            })
        })
    }
}
