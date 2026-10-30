//! Official PC search startup/suggestions: recovered M/623 and M/15.
mod project;
pub mod results;
use crate::{Error, Result, protocol::api::Api};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SearchDiscoveryQuery {
    Defaults,
    Recommendations { last_query: String },
    Charts,
    Chart { id: String },
    Suggestions { keyword: String },
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchWord {
    pub keyword: String,
    pub label: String,
    pub tag: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchChart {
    pub id: String,
    pub name: String,
    pub items: Vec<SearchWord>,
}
#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SearchDiscovery {
    Defaults {
        words: Vec<SearchWord>,
        refresh_ms: u64,
    },
    Recommendations {
        words: Vec<SearchWord>,
    },
    Charts {
        charts: Vec<SearchChart>,
    },
    Chart {
        id: String,
        items: Vec<SearchWord>,
    },
    Suggestions {
        title: String,
        recommended: Vec<SearchWord>,
        suggestions: Vec<SearchWord>,
    },
}
pub fn keyword(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 256 || value.chars().any(char::is_control) {
        return Err(Error::InvalidInput("invalid search keyword"));
    }
    Ok(value.to_owned())
}
pub async fn discover(api: Arc<dyn Api>, query: SearchDiscoveryQuery) -> Result<SearchDiscovery> {
    let (path, args) = match &query {
        SearchDiscoveryQuery::Defaults => ("/api/search/default/keyword/get", json!({})),
        SearchDiscoveryQuery::Recommendations { last_query } => {
            let last = if last_query.is_empty() {
                String::new()
            } else {
                keyword(last_query)?
            };
            ("/api/search/pc/rcmd/keyword/get", json!({"lastQuery":last}))
        }
        SearchDiscoveryQuery::Charts => ("/api/search/pc/chart/list", json!({})),
        SearchDiscoveryQuery::Chart { id } => {
            if !project::CHART_IDS.contains(&id.as_str()) {
                return Err(Error::InvalidInput("unknown search chart"));
            }
            ("/api/search/pc/chart/detail", json!({"id":id}))
        }
        SearchDiscoveryQuery::Suggestions { keyword: text } => (
            "/api/search/pc/suggest/keyword/get",
            json!({"keyword":keyword(text)?}),
        ),
    };
    let response = api.call(path, args, false, &[200]).await?;
    project::discovery(query, &response)
}
