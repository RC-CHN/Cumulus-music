//! Public audiobook catalogue and scoped podcast discovery continuations.
use super::{MusicClient, PodcastApi, PodcastCard, view};
use crate::{Error, Result, client::dto::AccountData, podcast::Radio};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::VecDeque, sync::Mutex};

const AUDIOBOOKS: &str = "10001";
const PAGE_SIZE: usize = 20;

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AudiobookDiscoveryQuery {
    Categories,
    Books {
        sub_id: String,
        continuation: Option<String>,
    },
}

#[derive(Serialize)]
pub struct AudiobookCategory {
    id: String,
    label: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudiobookDiscovery {
    Categories {
        categories: Vec<AudiobookCategory>,
    },
    Books {
        cards: Vec<PodcastCard>,
        continuation: Option<String>,
    },
}

struct Cursor {
    handle: String,
    epoch: String,
    scope: DiscoveryScope,
    value: String,
}

#[derive(Default)]
pub(in crate::client) struct PodcastDiscoveryHost(Mutex<VecDeque<Cursor>>);

#[derive(PartialEq, Eq)]
pub(super) enum DiscoveryScope {
    Audiobooks(String),
    PodcastHome,
}

impl PodcastDiscoveryHost {
    pub(super) fn resolve(
        &self,
        epoch: &str,
        scope: &DiscoveryScope,
        handle: Option<&str>,
    ) -> Result<String> {
        let Some(handle) = handle else {
            return Ok(String::new());
        };
        let rows = self
            .0
            .lock()
            .map_err(|_| Error::State("podcast discovery cursors unavailable"))?;
        rows.iter()
            .find(|row| row.handle == handle && row.epoch == epoch && &row.scope == scope)
            .map(|row| row.value.clone())
            .ok_or(Error::StaleResponse)
    }

    pub(super) fn save(&self, epoch: &str, scope: DiscoveryScope, value: String) -> Result<String> {
        let mut rows = self
            .0
            .lock()
            .map_err(|_| Error::State("podcast discovery cursors unavailable"))?;
        rows.retain(|row| row.epoch == epoch);
        if let Some(row) = rows
            .iter()
            .find(|row| row.scope == scope && row.value == value)
        {
            return Ok(row.handle.clone());
        }
        if rows.len() >= 64 {
            rows.pop_front();
        }
        let handle = uuid::Uuid::new_v4().simple().to_string();
        rows.push_back(Cursor {
            handle: handle.clone(),
            epoch: epoch.into(),
            scope,
            value,
        });
        Ok(handle)
    }
}

fn valid_sub_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_digit())
}

fn categories(rows: &[Value]) -> Result<Vec<AudiobookCategory>> {
    let category = rows
        .iter()
        .find(|row| row["id"] == AUDIOBOOKS)
        .ok_or(Error::Response("audiobook category unavailable"))?;
    let raw = category["subCategoryVO"]
        .as_array()
        .ok_or(Error::Response("audiobook subcategories missing"))?;
    let mut out: Vec<AudiobookCategory> = Vec::new();
    for row in raw.iter().take(100) {
        let Some(id) = row["id"].as_str().filter(|id| valid_sub_id(id)) else {
            continue;
        };
        let Some(label) = row["name"].as_str().map(str::trim).filter(|label| {
            !label.is_empty()
                && label.chars().count() <= 100
                && !label.chars().any(char::is_control)
        }) else {
            continue;
        };
        if !out.iter().any(|category| category.id == id) {
            out.push(AudiobookCategory {
                id: id.into(),
                label: label.into(),
            });
        }
    }
    if out.is_empty() {
        return Err(Error::Response("audiobook subcategories unavailable"));
    }
    Ok(out)
}

impl MusicClient {
    pub async fn audiobook_discovery(
        &self,
        epoch: &str,
        query: AudiobookDiscoveryQuery,
    ) -> Result<AccountData<AudiobookDiscovery>> {
        let token = self.podcast_token(epoch)?;
        let value = match query {
            AudiobookDiscoveryQuery::Categories => {
                let rows = self
                    .session
                    .run_for(&token, |api, _| async move {
                        PodcastApi::new(api).categories().await
                    })
                    .await?;
                AudiobookDiscovery::Categories {
                    categories: categories(&rows)?,
                }
            }
            AudiobookDiscoveryQuery::Books {
                sub_id,
                continuation,
            } => {
                if !valid_sub_id(&sub_id) {
                    return Err(Error::InvalidInput("invalid audiobook subcategory"));
                }
                let scope = DiscoveryScope::Audiobooks(sub_id.clone());
                let cursor =
                    self.podcast_discovery
                        .resolve(epoch, &scope, continuation.as_deref())?;
                let page = self
                    .session
                    .run_for(&token, |api, _| async {
                        PodcastApi::new(api)
                            .category(AUDIOBOOKS, &sub_id, &cursor, PAGE_SIZE)
                            .await
                    })
                    .await?;
                let cards = page
                    .records
                    .iter()
                    .map(|raw| Radio::from_official(raw).map(|radio| view::card(&radio)))
                    .collect::<Result<Vec<_>>>()?;
                let continuation = if page.more {
                    Some(self.session.with_token(&token, |_| {
                        self.podcast_discovery.save(epoch, scope, page.cursor)
                    })?)
                } else {
                    None
                };
                AudiobookDiscovery::Books {
                    cards,
                    continuation,
                }
            }
        };
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
