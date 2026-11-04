use super::*;
use crate::protocol::api::identifier;
use serde_json::Value;

// M/623 filters the server list to these recovered chart kinds.
pub(super) const CHART_IDS: &[&str] = &[
    "HOT_SEARCH_SONG#@#",
    "HOT_SEARCH_DJ_VOICE_BOOK#@#",
    "HOT_SEARCH_STYLE_RAP#@#",
    "HOT_SEARCH_STYLE_FOLKSONG#@#",
    "HOT_SEARCH_STYLE_ROCK#@#",
    "HOT_SEARCH_STYLE_TRADITION#@#",
    "HOT_SEARCH_STYLE_ACG#@#",
    "HOT_SEARCH_DJ#@#",
];
fn array(value: &Value) -> Result<&[Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or(Error::Response("missing search records"))
}
fn text(value: &Value) -> Option<String> {
    value.as_str().and_then(|s| keyword(s).ok())
}
fn word_tag(row: &Value) -> Option<String> {
    match row["iconType"].as_u64() {
        Some(1) => Some("热"),
        Some(2) => Some("新"),
        Some(3) => Some("荐"),
        Some(4) => Some("爆"),
        Some(5) => Some("升"),
        _ => None,
    }
    .map(str::to_owned)
}
fn word(row: &Value) -> Option<SearchWord> {
    let keyword = ["realkeyword", "searchWord", "keyword", "name"]
        .iter()
        .find_map(|k| text(&row[k]))?;
    let label = ["showKeyword", "toUserWord", "name"]
        .iter()
        .find_map(|k| text(&row[k]))
        .unwrap_or_else(|| keyword.clone());
    Some(SearchWord {
        keyword,
        label,
        tag: word_tag(row),
        channel: None,
        radio_id: None,
    })
}
fn words(rows: &[Value], limit: usize) -> Vec<SearchWord> {
    rows.iter().filter_map(word).take(limit).collect()
}
fn audio_chart(id: &str) -> bool {
    matches!(id, "HOT_SEARCH_DJ#@#" | "HOT_SEARCH_DJ_VOICE_BOOK#@#")
}

fn chart_radio_id(value: &Value) -> Option<String> {
    let raw = value
        .as_str()
        .filter(|url| url.len() <= 2048 && url.starts_with("orpheus://"))?;
    let url = url::Url::parse(raw).ok()?;
    let mut pairs = url.query_pairs();
    let (_, id) = pairs.find(|(key, _)| key == "id")?;
    if id.len() > 32 || pairs.any(|(key, _)| key == "id") {
        return None;
    }
    identifier(&Value::String(id.into_owned())).ok()
}

fn chart_words(rows: &[Value], audio: bool) -> Vec<SearchWord> {
    rows.iter()
        .filter_map(|row| {
            let keyword = ["searchWord", "keyword", "toUserWord", "name"]
                .into_iter()
                .find_map(|key| text(&row[key]))?;
            let label = text(&row["toUserWord"]).unwrap_or_else(|| keyword.clone());
            Some(SearchWord {
                keyword,
                label,
                tag: word_tag(row),
                channel: (!audio && row["tabId"] == "HOT_SEARCH_SONG#@#")
                    .then_some(results::SearchChannel::Hotquery),
                radio_id: audio.then(|| chart_radio_id(&row["url"])).flatten(),
            })
        })
        .take(CHART_ITEM_LIMIT)
        .collect()
}

fn chart_keywords(rows: &[Value]) -> Option<Vec<String>> {
    if rows.is_empty() || rows.len() > CHART_ITEM_LIMIT {
        return None;
    }
    rows.iter().map(|row| text(&row["searchWord"])).collect()
}
pub(super) fn discovery(query: SearchDiscoveryQuery, response: &Value) -> Result<SearchDiscovery> {
    let d = &response["data"];
    if !d.is_object() {
        return Err(Error::Response("missing search data"));
    }
    Ok(match query {
        SearchDiscoveryQuery::Defaults => SearchDiscovery::Defaults {
            words: words(array(&d["keywords"])?, 20),
            refresh_ms: d["refreshTime"].as_u64().unwrap_or(2500).clamp(2000, 60000),
        },
        SearchDiscoveryQuery::Recommendations { .. } => {
            let mut rows: Vec<Value> = d["operateWords"]
                .as_object()
                .map(|m| m.values().cloned().collect())
                .unwrap_or_default();
            rows.extend(array(&d["algWords"])?.iter().cloned());
            SearchDiscovery::Recommendations {
                words: words(&rows, 40),
            }
        }
        SearchDiscoveryQuery::Charts => {
            let mut charts = Vec::new();
            for row in array(&d["tabInfo"]["list"])? {
                let Some(id) = row["id"].as_str().filter(|s| CHART_IDS.contains(s)) else {
                    continue;
                };
                let Some(name) = text(&row["name"]) else {
                    continue;
                };
                let rows = if d["items"][id].is_null() {
                    &[]
                } else {
                    array(&d["items"][id])?
                };
                let audio = audio_chart(id);
                let need_play = !audio && row["needPlay"] == true;
                let items = chart_words(rows, audio);
                if !charts.iter().any(|c: &SearchChart| c.id == id) {
                    charts.push(SearchChart {
                        id: id.into(),
                        name,
                        items,
                        play_keywords: if need_play {
                            chart_keywords(rows)
                        } else {
                            None
                        },
                        need_play,
                        audio,
                        audiobook_more: (id == "HOT_SEARCH_DJ_VOICE_BOOK#@#")
                            .then(|| text(&row["button"]["text"]))
                            .flatten(),
                    });
                }
            }
            SearchDiscovery::Charts { charts }
        }
        SearchDiscoveryQuery::Chart { id } => {
            let rows = array(&d["itemList"])?;
            let audio = audio_chart(&id);
            SearchDiscovery::Chart {
                id,
                items: chart_words(rows, audio),
                play_keywords: (!audio).then(|| chart_keywords(rows)).flatten(),
            }
        }
        SearchDiscoveryQuery::ChartSongs { .. } => {
            let rows = array(&d["songs"])?;
            if rows.is_empty() || rows.len() > CHART_ITEM_LIMIT {
                return Err(Error::Response("invalid search chart songs"));
            }
            let ids = rows
                .iter()
                .map(|row| identifier(&row["song"]["id"]))
                .collect::<Result<Vec<_>>>()?;
            SearchDiscovery::ChartSongs { ids }
        }
        SearchDiscoveryQuery::Suggestions { .. } => SearchDiscovery::Suggestions {
            title: text(&d["recTitle"]).unwrap_or_else(|| "猜你想搜".into()),
            recommended: words(array(&d["recs"])?, 10),
            suggestions: words(array(&d["suggests"])?, 20),
        },
    })
}
