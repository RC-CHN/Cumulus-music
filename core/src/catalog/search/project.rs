use super::*;
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
fn word(row: &Value) -> Option<SearchWord> {
    let keyword = ["realkeyword", "searchWord", "keyword", "name"]
        .iter()
        .find_map(|k| text(&row[k]))?;
    let label = ["showKeyword", "toUserWord", "name"]
        .iter()
        .find_map(|k| text(&row[k]))
        .unwrap_or_else(|| keyword.clone());
    let tag = match row["iconType"].as_u64() {
        Some(1) => Some("热"),
        Some(2) => Some("新"),
        Some(3) => Some("荐"),
        Some(4) => Some("爆"),
        Some(5) => Some("升"),
        _ => None,
    }
    .map(str::to_owned);
    Some(SearchWord {
        keyword,
        label,
        tag,
    })
}
fn words(rows: &[Value], limit: usize) -> Vec<SearchWord> {
    rows.iter().filter_map(word).take(limit).collect()
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
                let items = if d["items"][id].is_null() {
                    vec![]
                } else {
                    words(array(&d["items"][id])?, 20)
                };
                if !charts.iter().any(|c: &SearchChart| c.id == id) {
                    charts.push(SearchChart {
                        id: id.into(),
                        name,
                        items,
                    });
                }
            }
            SearchDiscovery::Charts { charts }
        }
        SearchDiscoveryQuery::Chart { id } => SearchDiscovery::Chart {
            id,
            items: words(array(&d["itemList"])?, 20),
        },
        SearchDiscoveryQuery::Suggestions { .. } => SearchDiscovery::Suggestions {
            title: text(&d["recTitle"]).unwrap_or_else(|| "猜你想搜".into()),
            recommended: words(array(&d["recs"])?, 10),
            suggestions: words(array(&d["suggests"])?, 20),
        },
    })
}
