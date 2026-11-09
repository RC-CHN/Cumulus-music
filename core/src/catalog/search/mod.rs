//! Official PC search startup/suggestions: recovered M/623 and M/15.
mod project;
pub mod results;
mod suggestion_marks;
use crate::{Error, Result, protocol::api::Api};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{future::Future, sync::Arc, time::Duration};

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
    ChartSongs { keywords: Vec<String> },
    Suggestions { keyword: String },
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchWord {
    pub keyword: String,
    pub label: String,
    pub tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<results::SearchChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radio_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_receipt: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSuggestionWord {
    #[serde(flatten)]
    pub word: SearchWord,
    // The confirmed suggestion can fill different text into the input than
    // the popup displayed. Omit the override when the strings are identical.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_label: Option<String>,
    pub label_parts: Vec<SearchSuggestionPart>,
}

#[derive(Clone, Serialize)]
pub struct SearchSuggestionPart {
    pub text: String,
    #[serde(rename = "match")]
    pub marked: bool,
}

pub(crate) fn song_table_alias_parts(aliases: &[&str], query: &str) -> Vec<SearchSuggestionPart> {
    suggestion_marks::first_alias(aliases, query)
        .into_iter()
        .map(|part| SearchSuggestionPart {
            text: part.text.into(),
            marked: part.marked,
        })
        .collect()
}

pub(crate) struct DefaultWord {
    pub word: SearchWord,
    pub context: Option<Value>,
}

const DEFAULT_PATH: &str = "/api/search/default/keyword/get";

pub(crate) async fn fetch_default_words(api: &dyn Api) -> Result<(Vec<DefaultWord>, u64)> {
    let response = api.call(DEFAULT_PATH, json!({}), false, &[200]).await?;
    project::default_words(&response)
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchChart {
    pub id: String,
    pub name: String,
    pub items: Vec<SearchWord>,
    pub play_keywords: Option<Vec<String>>,
    pub need_play: bool,
    pub audio: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more: Option<SearchChartMore>,
}
#[derive(Clone, Serialize)]
pub struct SearchChartMore {
    pub label: String,
    pub target: SearchChartDestination,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchChartDestination {
    Podcasts,
    Audiobooks,
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
        play_keywords: Option<Vec<String>>,
    },
    ChartSongs {
        ids: Vec<String>,
    },
    Suggestions {
        title: String,
        recommended: Vec<SearchSuggestionWord>,
        suggestions: Vec<SearchSuggestionWord>,
    },
}
pub(crate) const CHART_ITEM_LIMIT: usize = 20;

pub fn keyword(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 256 || value.chars().any(char::is_control) {
        return Err(Error::InvalidInput("invalid search keyword"));
    }
    Ok(value.to_owned())
}
pub async fn discover(api: Arc<dyn Api>, query: SearchDiscoveryQuery) -> Result<SearchDiscovery> {
    discover_for(&api, query, || async {
        Ok(tokio::time::timeout(
            Duration::from_secs(8),
            api.search_audio_toplist_assignment(),
        )
        .await
        .is_ok_and(|assignment| matches!(assignment, Ok(true))))
    })
    .await
}

pub(crate) async fn discover_for<Fut: Future<Output = Result<bool>>>(
    api: &Arc<dyn Api>,
    mut query: SearchDiscoveryQuery,
    audio_toplist: impl FnOnce() -> Fut,
) -> Result<SearchDiscovery> {
    // Use the exact validated wire keyword for both the request and the
    // local label matcher. Mark normalization never rewrites the request.
    if let SearchDiscoveryQuery::Suggestions { keyword: text } = &mut query {
        *text = keyword(text)?;
    }
    let (path, args) = match &query {
        SearchDiscoveryQuery::Defaults => {
            let (words, refresh_ms) = fetch_default_words(api.as_ref()).await?;
            return Ok(SearchDiscovery::Defaults {
                words: words.into_iter().map(|row| row.word).collect(),
                refresh_ms,
            });
        }
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
        SearchDiscoveryQuery::ChartSongs { keywords } => {
            if keywords.is_empty() || keywords.len() > CHART_ITEM_LIMIT {
                return Err(Error::InvalidInput("invalid search chart keywords"));
            }
            let words = keywords
                .iter()
                .map(|word| keyword(word))
                .collect::<Result<Vec<_>>>()?;
            (
                "/api/hot/search/relate/song/get",
                json!({"keywords":serde_json::to_string(&words)?}),
            )
        }
        SearchDiscoveryQuery::Suggestions { keyword: text } => (
            "/api/search/pc/suggest/keyword/get",
            json!({"keyword":text}),
        ),
    };
    if matches!(&query, SearchDiscoveryQuery::Chart { id } if project::audio_chart(id)) {
        if !audio_toplist().await? {
            return Err(Error::InvalidInput("search audio chart unavailable"));
        }
        let response = api.call(path, args, false, &[200]).await?;
        return project::discovery(query, &response, true);
    }
    let response = api.call(path, args, false, &[200]).await?;
    // M/623 reads chart/list before consulting the assignment. An ordinary
    // list needs no extra lookup, and hidden rows never enter projection.
    let include_audio = matches!(&query, SearchDiscoveryQuery::Charts)
        && project::has_audio_chart(&response)
        && audio_toplist().await?;
    project::discovery(query, &response, include_audio)
}
