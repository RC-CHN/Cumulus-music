//! Public podcast categories and scoped discovery continuations.
use super::{MusicClient, PodcastApi, PodcastCard, view};
use crate::{
    Error, Result,
    client::dto::AccountData,
    podcast::Radio,
    protocol::wire::serialization::{js_string, js_truthy},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::VecDeque, sync::Mutex};

const PAGE_SIZE: usize = 20;

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PodcastCatalogueQuery {
    Categories,
    Radios {
        parent_id: String,
        sub_id: String,
        continuation: Option<String>,
    },
}

#[derive(Serialize)]
pub struct PodcastSubcategory {
    pub id: String,
    pub label: String,
}

#[derive(Serialize)]
pub struct PodcastCategory {
    pub id: String,
    pub label: String,
    pub children: Vec<PodcastSubcategory>,
}

#[derive(Serialize)]
pub struct PodcastCatalogueCard {
    #[serde(flatten)]
    pub card: PodcastCard,
    pub score: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PodcastCatalogue {
    Categories {
        categories: Vec<PodcastCategory>,
    },
    Radios {
        cards: Vec<PodcastCatalogueCard>,
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
    Category { parent_id: String, sub_id: String },
    PodcastHome(uuid::Uuid),
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

    pub(super) fn forget(&self, epoch: &str, scope: &DiscoveryScope) -> Result<()> {
        self.0
            .lock()
            .map_err(|_| Error::State("podcast discovery cursors unavailable"))?
            .retain(|row| row.epoch != epoch || &row.scope != scope);
        Ok(())
    }
}

fn valid_category_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_digit())
}

fn category_name(row: &Value) -> Option<(&str, &str)> {
    let id = row["id"].as_str().filter(|id| valid_category_id(id))?;
    let label = row["name"].as_str().map(str::trim).filter(|label| {
        !label.is_empty() && label.chars().count() <= 100 && !label.chars().any(char::is_control)
    })?;
    Some((id, label))
}

fn categories(rows: &[Value]) -> Result<Vec<PodcastCategory>> {
    let mut out: Vec<PodcastCategory> = Vec::new();
    for row in rows.iter().take(100) {
        let Some((id, label)) = category_name(row) else {
            continue;
        };
        if out.iter().any(|category| category.id == id) {
            continue;
        }
        let Some(raw) = row["subCategoryVO"].as_array() else {
            continue;
        };
        let mut children: Vec<PodcastSubcategory> = Vec::new();
        for (id, label) in raw.iter().take(100).filter_map(category_name) {
            if !children.iter().any(|category| category.id == id) {
                children.push(PodcastSubcategory {
                    id: id.into(),
                    label: label.into(),
                });
            }
        }
        if !children.is_empty() {
            out.push(PodcastCategory {
                id: id.into(),
                label: label.into(),
                children,
            });
        }
    }
    if out.is_empty() {
        return Err(Error::Response("podcast categories unavailable"));
    }
    Ok(out)
}

// M/21 Z uses score || null, then M/2169 displays a truthy value with 分.
fn category_score(value: &Value) -> Option<String> {
    match value {
        Value::Number(_) if js_truthy(value) => js_string(value).ok(),
        Value::String(text)
            if text.len() <= 32
                && text.bytes().all(|byte| {
                    byte.is_ascii_digit() || matches!(byte, b'.' | b'+' | b'-' | b'e' | b'E')
                })
                && text.parse::<f64>().is_ok_and(f64::is_finite) =>
        {
            Some(text.clone())
        }
        _ => None,
    }
}

impl MusicClient {
    pub async fn podcast_catalogue(
        &self,
        epoch: &str,
        query: PodcastCatalogueQuery,
    ) -> Result<AccountData<PodcastCatalogue>> {
        let token = self.podcast_token(epoch)?;
        let value = match query {
            PodcastCatalogueQuery::Categories => {
                let rows = self
                    .session
                    .run_for(&token, |api, _| async move {
                        PodcastApi::new(api).categories().await
                    })
                    .await?;
                PodcastCatalogue::Categories {
                    categories: categories(&rows)?,
                }
            }
            PodcastCatalogueQuery::Radios {
                parent_id,
                sub_id,
                continuation,
            } => {
                if !valid_category_id(&parent_id) || !valid_category_id(&sub_id) {
                    return Err(Error::InvalidInput("invalid podcast category"));
                }
                let scope = DiscoveryScope::Category {
                    parent_id: parent_id.clone(),
                    sub_id: sub_id.clone(),
                };
                let cursor =
                    self.podcast_discovery
                        .resolve(epoch, &scope, continuation.as_deref())?;
                let page = self
                    .session
                    .run_for(&token, |api, _| async {
                        PodcastApi::new(api)
                            .category(&parent_id, &sub_id, &cursor, PAGE_SIZE)
                            .await
                    })
                    .await?;
                let cards = page
                    .records
                    .iter()
                    .map(|raw| {
                        let radio = Radio::from_official(raw)?;
                        Ok(PodcastCatalogueCard {
                            card: view::card(&radio),
                            score: category_score(&radio.raw["score"]),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let continuation = if page.more {
                    Some(self.session.with_token(&token, |_| {
                        self.podcast_discovery.save(epoch, scope, page.cursor)
                    })?)
                } else {
                    None
                };
                PodcastCatalogue::Radios {
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
