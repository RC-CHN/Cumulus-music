//! Read-only style cards projected from the official PC daily mix.
use super::{MusicClient, dto::AccountData, mapping};
use crate::{Error, Result, account::SessionToken, catalog::CatalogApi, protocol::api::identifier};
use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Mutex};

const MAX_CARDS: usize = 12;
const MAX_ROWS: usize = 128;
const MAX_SONG_IDS: usize = 32;

#[derive(Clone, Deserialize, Serialize, Hash, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StyleSeedHandle {
    revision: String,
    id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleSeedCard {
    id: String,
    title: String,
    cover_url: Option<String>,
}

#[derive(Serialize)]
pub struct StyleSeedPage {
    revision: String,
    cards: Vec<StyleSeedCard>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub(super) struct StyleSeed {
    category_id: String,
    tag_id: String,
    song_id: String,
}

impl StyleSeed {
    pub(super) fn request(&self) -> Value {
        json!({"source":"homepage","categoryId":self.category_id,"tagId":self.tag_id,"songId":self.song_id})
    }
}

#[derive(Default)]
pub(super) struct StyleHome {
    state: Mutex<SeedState>,
}

#[derive(Default)]
struct SeedState {
    started: u64,
    latest: Option<SeedSnapshot>,
    discovery_started: u64,
    discovery: Option<SeedSnapshot>,
}

struct SeedSnapshot {
    token: SessionToken,
    revision: String,
    seeds: Vec<(String, StyleSeed)>,
}

pub(super) struct DiscoveryStyleSeeds {
    revision: String,
    seeds: Vec<(String, StyleSeed)>,
}

impl Default for DiscoveryStyleSeeds {
    fn default() -> Self {
        Self {
            revision: uuid::Uuid::new_v4().simple().to_string(),
            seeds: Vec::new(),
        }
    }
}

impl DiscoveryStyleSeeds {
    pub(super) fn add(&mut self, item: &Value) -> Option<StyleSeedHandle> {
        let seed = from_item(item)?;
        let id = if let Some((id, _)) = self.seeds.iter().find(|(_, old)| old == &seed) {
            id.clone()
        } else if self.seeds.len() < MAX_CARDS {
            let id = uuid::Uuid::new_v4().simple().to_string();
            self.seeds.push((id.clone(), seed));
            id
        } else {
            return None;
        };
        Some(StyleSeedHandle {
            revision: self.revision.clone(),
            id,
        })
    }
}

impl StyleHome {
    pub(super) fn begin_discovery(&self) -> Result<u64> {
        let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        state.discovery_started = state
            .discovery_started
            .checked_add(1)
            .ok_or(Error::State("discovery style revision exhausted"))?;
        Ok(state.discovery_started)
    }

    pub(super) fn publish_discovery(
        &self,
        token: &SessionToken,
        request: u64,
        cards: DiscoveryStyleSeeds,
    ) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        if state.discovery_started != request {
            return Err(Error::StaleResponse);
        }
        state.discovery = (!cards.seeds.is_empty()).then(|| SeedSnapshot {
            token: token.clone(),
            revision: cards.revision,
            seeds: cards.seeds,
        });
        Ok(())
    }
}

fn key(value: &Value) -> Option<String> {
    let id = if let Some(id) = value.as_str() {
        id.to_owned()
    } else {
        value
            .as_u64()
            .filter(|id| *id <= 9_007_199_254_740_991)?
            .to_string()
    };
    (!id.trim().is_empty() && id.len() <= 80 && !id.chars().any(char::is_control)).then_some(id)
}

fn decimal(value: &Value) -> Option<String> {
    if value.as_u64().is_some_and(|id| id > 9_007_199_254_740_991) {
        return None;
    }
    identifier(value).ok().filter(|id| id.len() <= 32)
}

fn join_songs(ids: impl Iterator<Item = Option<String>>) -> Option<String> {
    let ids = ids.take(MAX_SONG_IDS + 1).collect::<Option<Vec<_>>>()?;
    (!ids.is_empty() && ids.len() <= MAX_SONG_IDS).then(|| ids.join(","))
}

fn songs(item: &Value) -> Option<String> {
    if let Some(target) = item["targetUrl"]
        .as_str()
        .filter(|target| !target.is_empty())
    {
        if target.len() > 4096 {
            return None;
        }
        let parsed = url::Url::parse(target).ok()?;
        if let Some((_, value)) = parsed.query_pairs().find(|(key, _)| key == "songId")
            && !value.is_empty()
        {
            // The route is parsed as data only; it is never visited or exposed.
            return join_songs(value.split(',').map(|part| decimal(&json!(part))));
        }
    }
    join_songs(item["songIds"].as_array()?.iter().map(decimal))
}

fn from_item(item: &Value) -> Option<StyleSeed> {
    if item["resourceType"] != "dailySongs" || item["subResourceType"] != "style_dailySong" {
        return None;
    }
    Some(StyleSeed {
        category_id: key(&item["extData"]["categoryId"])?,
        tag_id: key(&item["extData"]["tagId"])?,
        song_id: songs(item)?,
    })
}

fn project(response: &Value) -> Result<Vec<(StyleSeedCard, StyleSeed)>> {
    let items = response["data"]["items"]
        .as_array()
        .ok_or(Error::Response("missing homepage daily mix items"))?;
    let demotes = response["data"]["extInfo"]["pcRcmdDemoteItems"]
        .as_array()
        .into_iter()
        .flatten();
    let mut seen = HashSet::new();
    Ok(items
        .iter()
        .take(MAX_ROWS)
        .filter(|item| item["moduleType"] == "tag_daily_rec")
        .chain(demotes.take(MAX_ROWS))
        .filter_map(|item| {
            let seed = from_item(item)?;
            let title = item["title"]
                .as_str()
                .or_else(|| item["coverText"].as_str())?
                .trim();
            if title.is_empty()
                || title.chars().count() > 100
                || title.chars().any(char::is_control)
                || !seen.insert(seed.clone())
            {
                return None;
            }
            Some((
                StyleSeedCard {
                    id: uuid::Uuid::new_v4().simple().to_string(),
                    title: title.into(),
                    cover_url: mapping::artwork(&item["coverUrl"]),
                },
                seed,
            ))
        })
        .take(MAX_CARDS)
        .collect())
}

impl MusicClient {
    pub async fn style_home_seeds(&self, epoch: &str) -> Result<AccountData<StyleSeedPage>> {
        let token = self.player_token(epoch)?;
        let request = self.session.with_token(&token, |_| {
            let mut state = self
                .style_home
                .state
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            state.started = state
                .started
                .checked_add(1)
                .ok_or(Error::State("style home revision exhausted"))?;
            Ok(state.started)
        })?;
        let cards = self
            .session
            .run_for(&token, |api, _| async move {
                let client_time = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                let response = CatalogApi::new(api).daily_mix(&client_time).await?;
                project(&response)
            })
            .await?;
        self.session.with_token(&token, |_| {
            let mut state = self
                .style_home
                .state
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if state.started != request {
                return Err(Error::StaleResponse);
            }
            let revision = uuid::Uuid::new_v4().simple().to_string();
            let seeds = cards
                .iter()
                .map(|(card, seed)| (card.id.clone(), seed.clone()))
                .collect();
            state.latest = Some(SeedSnapshot {
                token: token.clone(),
                revision: revision.clone(),
                seeds,
            });
            Ok(AccountData {
                epoch: epoch.into(),
                value: StyleSeedPage {
                    revision,
                    cards: cards.into_iter().map(|(card, _)| card).collect(),
                },
            })
        })
    }

    /// Caller holds the current session fence. Cards from old feeds/accounts
    /// and IDs fabricated by the renderer cannot supply request parameters.
    pub(super) fn style_seed(
        &self,
        token: &SessionToken,
        handle: &StyleSeedHandle,
    ) -> Result<StyleSeed> {
        let state = self
            .style_home
            .state
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        [state.latest.as_ref(), state.discovery.as_ref()]
            .into_iter()
            .flatten()
            .find(|snapshot| snapshot.token == *token && snapshot.revision == handle.revision)
            .and_then(|snapshot| snapshot.seeds.iter().find(|(id, _)| id == &handle.id))
            .map(|(_, seed)| seed.clone())
            .ok_or(Error::QueueChanged)
    }
}
