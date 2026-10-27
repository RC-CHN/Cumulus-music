use super::*;
use crate::protocol::api::identifier;
use serde_json::Value;
fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}
fn card(v: &Value, source: FmSource) -> Option<FmCard> {
    source.validate().ok()?;
    let title = v["coverText"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| v["title"].as_str())
        .unwrap_or_default()
        .to_owned();
    Some(FmCard {
        source,
        title,
        subtitle: v["simplifiedTitle"]
            .as_str()
            .unwrap_or_else(|| v["title"].as_str().unwrap_or_default())
            .into(),
        cover_url: v["coverUrl"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
    })
}
pub(super) fn home(v: &Value, tabs: &Value) -> Result<FmDiscoveryPage> {
    let items = v["data"]["items"]
        .as_array()
        .ok_or(Error::Response("missing FM feature cards"))?;
    let mut cards = Vec::new();
    for item in items {
        let source = match item["resourceType"].as_str() {
            Some("fm") => Some(FmSource::Personal),
            Some("similarSong") => item["resourceId"].as_str().map(|s| FmSource::Familiar {
                source_id: s.into(),
            }),
            Some("zone") => identifier(&item["extData"]["firstFm"]["resourceId"])
                .ok()
                .map(|rule_id| FmSource::Zone {
                    rule_id,
                    trial_mode: None,
                }),
            _ => None,
        };
        if let Some(card) = source.and_then(|s| card(item, s))
            && !cards.iter().any(|c: &FmCard| c.source == card.source)
        {
            cards.push(card);
        }
    }
    let hints = tabs["data"]["hints"]
        .as_array()
        .ok_or(Error::Response("missing FM zone navigation"))?;
    let mut zones = Vec::new();
    if let Some(rows) = hints
        .iter()
        .find(|h| h["position"]["code"] == "music_top_tab_full")
        .and_then(|h| h["data"]["extra"]["nowChannelItems"].as_array())
    {
        for row in rows {
            if matches!(
                row["code"].as_str(),
                Some("feature" | "playlist" | "chart" | "singer" | "vip" | "origin")
            ) {
                continue;
            }
            let Some(url) = row["url"].as_str().and_then(|s| url::Url::parse(s).ok()) else {
                continue;
            };
            let Some(page_code) = url
                .query_pairs()
                .find(|(k, _)| k == "page")
                .map(|(_, v)| v.into_owned())
            else {
                continue;
            };
            if (FmDiscoveryQuery::Zone {
                page_code: page_code.clone(),
                tab_key: "default".into(),
            })
            .validate()
            .is_err()
            {
                continue;
            }
            if !zones.iter().any(|z: &FmZone| z.page_code == page_code) {
                zones.push(FmZone {
                    page_code,
                    name: text(&row["title"]),
                });
            }
        }
    }
    Ok(FmDiscoveryPage {
        cards,
        zones,
        ..Default::default()
    })
}
pub(super) fn zone(v: &Value, requested: &str, fallback: bool) -> Result<FmDiscoveryPage> {
    let content = if fallback {
        &v["data"][0]
    } else {
        &v["data"]["currentTabContent"]
    };
    let tab_key = content["tabKey"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or(Error::Response("missing FM zone content"))?;
    if !fallback && tab_key != requested {
        return Err(Error::Response("FM zone tab mismatch"));
    }
    let blocks = content["blocks"]
        .as_array()
        .ok_or(Error::Response("missing FM zone blocks"))?;
    let mut cards = Vec::new();
    for block in blocks {
        if block["componentName"] != "RecommendHorizontal" {
            continue;
        }
        for row in block
            .get("children")
            .or_else(|| block.get("childrens"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if row["resourceType"] != "fm" {
                continue;
            }
            if let Ok(rule_id) = identifier(&row["resourceId"])
                && let Some(card) = card(
                    row,
                    FmSource::Zone {
                        rule_id,
                        trial_mode: None,
                    },
                )
                && !cards.iter().any(|c: &FmCard| c.source == card.source)
            {
                cards.push(card);
            }
        }
    }
    let tabs = if fallback {
        Vec::new()
    } else {
        v["data"]["tabMetas"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| {
                let key = t["tabKey"].as_str().filter(|s| {
                    !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
                })?;
                Some(FmZoneTab {
                    key: key.into(),
                    name: text(&t["tabKeyName"]),
                })
            })
            .collect()
    };
    Ok(FmDiscoveryPage {
        cards,
        tabs,
        active_tab: Some(tab_key.into()),
        default_fallback: fallback,
        ..Default::default()
    })
}
