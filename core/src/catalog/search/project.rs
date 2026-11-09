use super::*;
use crate::protocol::api::identifier;
use crate::protocol::wire::serialization::js_truthy;
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
        default_receipt: None,
    })
}
fn words(rows: &[Value], limit: usize) -> Vec<SearchWord> {
    rows.iter().filter_map(word).take(limit).collect()
}

fn suggestion_text(row: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .map(|key| &row[*key])
        .find(|value| js_truthy(value))
        .and_then(text)
}

fn suggestion_words(
    rows: &[Value],
    limit: usize,
    matcher: &suggestion_marks::SuggestionMatcher,
) -> Vec<SearchSuggestionWord> {
    rows.iter()
        .filter_map(|row| {
            // PC M/623 uses independent values for the visible row, the
            // requested query, and the text left in the input after selection.
            let keyword = suggestion_text(row, &["searchWord", "keyword", "toUserWord", "name"])?;
            let input = suggestion_text(row, &["toUserWord", "searchWord", "keyword", "name"])
                .unwrap_or_else(|| keyword.clone());
            let label = suggestion_text(row, &["name", "keyword"]).unwrap_or_else(|| input.clone());
            let input_label = (input != label).then_some(input);
            let word = SearchWord {
                keyword,
                label,
                tag: word_tag(row),
                channel: None,
                radio_id: None,
                default_receipt: None,
            };
            Some((word, input_label))
        })
        .take(limit)
        .map(|(word, input_label)| {
            let label_parts = matcher
                .parts(&word.label)
                .into_iter()
                .map(|part| SearchSuggestionPart {
                    text: part.text.to_owned(),
                    marked: part.marked,
                })
                .collect();
            SearchSuggestionWord {
                word,
                input_label,
                label_parts,
            }
        })
        .collect()
}

pub(super) fn default_words(response: &Value) -> Result<(Vec<DefaultWord>, u64)> {
    let data = &response["data"];
    if !data.is_object() {
        return Err(Error::Response("missing search data"));
    }
    let refresh_ms = data["refreshTime"]
        .as_u64()
        .unwrap_or(2500)
        .clamp(2000, 60000);
    let words = array(&data["keywords"])?
        .iter()
        .filter_map(|row| {
            let word = word(row)?;
            // A context belongs to one actual default response row. A
            // compatibility name-only row cannot assert official provenance.
            let context = (text(&row["realkeyword"]).as_deref() == Some(word.keyword.as_str()))
                .then(|| row.get("bizQueryInfo"))
                .flatten()
                .filter(|value| {
                    crate::protocol::wire::serialization::js_truthy(value)
                        && value.to_string().len() <= 4096
                })
                .cloned();
            Some(DefaultWord { word, context })
        })
        .take(20)
        .collect();
    Ok((words, refresh_ms))
}
pub(super) fn audio_chart(id: &str) -> bool {
    matches!(id, "HOT_SEARCH_DJ#@#" | "HOT_SEARCH_DJ_VOICE_BOOK#@#")
}

pub(super) fn has_audio_chart(response: &Value) -> bool {
    response["data"]["tabInfo"]["list"]
        .as_array()
        .is_some_and(|rows| {
            rows.iter().any(|row| {
                row["id"].as_str().is_some_and(audio_chart) && text(&row["name"]).is_some()
            })
        })
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
                default_receipt: None,
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
pub(super) fn discovery(
    query: SearchDiscoveryQuery,
    response: &Value,
    include_audio: bool,
) -> Result<SearchDiscovery> {
    let d = &response["data"];
    if !d.is_object() {
        return Err(Error::Response("missing search data"));
    }
    Ok(match query {
        SearchDiscoveryQuery::Defaults => unreachable!("defaults have their own native projection"),
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
                let audio = audio_chart(id);
                if audio && !include_audio {
                    continue;
                }
                let rows = if d["items"][id].is_null() {
                    &[]
                } else {
                    array(&d["items"][id])?
                };
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
                        more: text(&row["button"]["text"]).and_then(|label| {
                            let target = match id {
                                "HOT_SEARCH_DJ#@#" => SearchChartDestination::Podcasts,
                                "HOT_SEARCH_DJ_VOICE_BOOK#@#" => SearchChartDestination::Audiobooks,
                                _ => return None,
                            };
                            Some(SearchChartMore { label, target })
                        }),
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
        SearchDiscoveryQuery::Suggestions { keyword } => {
            let matcher = suggestion_marks::SuggestionMatcher::new(&keyword);
            SearchDiscovery::Suggestions {
                title: text(&d["recTitle"]).unwrap_or_else(|| "猜你想搜".into()),
                recommended: suggestion_words(array(&d["recs"])?, 10, &matcher),
                suggestions: suggestion_words(array(&d["suggests"])?, 20, &matcher),
            }
        }
    })
}
