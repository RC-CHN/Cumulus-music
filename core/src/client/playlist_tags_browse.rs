use super::{MusicClient, dto::AccountData, mapping};
use crate::{Error, Result, account::AccountApi, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

const LIMIT: usize = 24;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaylistTagQuery {
    Category { tag: String },
    Items { ids: Vec<String> },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistTagPage {
    pub items: Vec<PlaylistTagCard>,
    pub remaining_ids: Vec<String>,
    pub essential: Option<PlaylistEssentialPromo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEssentialPromo {
    name: String,
    cover_url: Option<String>,
}

fn essential_promo(value: &Value) -> Option<PlaylistEssentialPromo> {
    let name = value["name"].as_str()?.trim();
    if name.is_empty() || name.chars().count() > 200 || name.chars().any(char::is_control) {
        return None;
    }
    Some(PlaylistEssentialPromo {
        name: name.into(),
        cover_url: mapping::artwork(&value["picUrl"]),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistTagCard {
    pub id: String,
    pub name: String,
    pub cover_url: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistTagGroup {
    id: u8,
    name: String,
    tags: Vec<PlaylistCatalogueTag>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistTopTags {
    recommendation_enabled: bool,
    tags: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistCatalogueTag {
    name: String,
    display_name: Option<String>,
}

pub(super) fn safe_label(label: &str) -> bool {
    !label.is_empty()
        && label.trim() == label
        && label.chars().count() <= 80
        && !label.chars().any(char::is_control)
}

fn valid_tag(label: &str) -> bool {
    safe_label(label) && label != "推荐"
}

fn top_tags(reply: &Value) -> Result<Vec<String>> {
    let rows = reply["data"]
        .as_array()
        .filter(|rows| rows.len() <= 256)
        .ok_or(Error::Response("invalid playlist top tags"))?;
    let mut seen = HashSet::new();
    Ok(rows
        .iter()
        .filter_map(|row| row["tag"].as_str())
        .filter(|tag| valid_tag(tag) && *tag != "官方" && seen.insert(*tag))
        .take(16)
        .map(str::to_owned)
        .collect())
}

fn catalogue(reply: &Value) -> Result<Vec<PlaylistTagGroup>> {
    let categories = reply["categories"]
        .as_object()
        .filter(|value| value.len() <= 32)
        .ok_or(Error::Response("invalid playlist tag catalogue groups"))?;
    let tags = reply["sub"]
        .as_array()
        .filter(|value| value.len() <= 256)
        .ok_or(Error::Response("invalid playlist tag catalogue entries"))?;
    let mut seen = HashSet::new();
    let mut keys: Vec<_> = categories.keys().collect();
    keys.sort_unstable();
    Ok(keys
        .into_iter()
        .filter_map(|key| {
            let id = key.parse::<u8>().ok().filter(|id| id.to_string() == *key)?;
            let name = categories[key]
                .as_str()
                .filter(|name| safe_label(name))?
                .to_owned();
            let tags = tags
                .iter()
                .filter(|tag| tag["category"] == id)
                .filter_map(|tag| {
                    let name = tag["name"]
                        .as_str()
                        .filter(|name| valid_tag(name) && *name != "官方" && seen.insert(*name))?;
                    let display_name = tag["displayName"]
                        .as_str()
                        .filter(|value| safe_label(value))
                        .map(str::to_owned);
                    Some(PlaylistCatalogueTag {
                        name: name.to_owned(),
                        display_name,
                    })
                })
                .collect::<Vec<_>>();
            (!tags.is_empty()).then_some(PlaylistTagGroup { id, name, tags })
        })
        .collect())
}

impl MusicClient {
    pub async fn playlist_top_tags(&self, epoch: &str) -> Result<AccountData<PlaylistTopTags>> {
        let token = self.playlist_token(epoch)?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                if AccountApi::new(api.clone()).settings().await?["personalServiceSetting"] == 2 {
                    return Ok(PlaylistTopTags {
                        recommendation_enabled: false,
                        tags: Vec::new(),
                    });
                }
                let reply = api
                    .call(
                        "/api/personalized/playlist/tag/rcmd",
                        json!({}),
                        false,
                        &[200],
                    )
                    .await?;
                Ok(PlaylistTopTags {
                    recommendation_enabled: true,
                    tags: top_tags(&reply)?,
                })
            })
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub async fn playlist_tag_catalogue(
        &self,
        epoch: &str,
    ) -> Result<AccountData<Vec<PlaylistTagGroup>>> {
        let token = self.playlist_token(epoch)?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                let reply = api
                    .call("/api/playlist/catalogue", json!({}), false, &[200])
                    .await?;
                catalogue(&reply)
            })
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub async fn playlist_tag_page(
        &self,
        epoch: &str,
        query: PlaylistTagQuery,
    ) -> Result<AccountData<PlaylistTagPage>> {
        let token = self.playlist_token(epoch)?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                let (path, body, requested) = match query {
                    PlaylistTagQuery::Category { tag } => {
                        if !valid_tag(&tag) {
                            return Err(Error::InvalidInput("invalid playlist category"));
                        }
                        (
                            "/api/playlist/category/list",
                            json!({"cat":tag,"limit":LIMIT,"newStyle":true}),
                            None,
                        )
                    }
                    PlaylistTagQuery::Items { ids } => {
                        if ids.is_empty() || ids.len() > LIMIT || !distinct(&ids) {
                            return Err(Error::InvalidInput("invalid category playlist page"));
                        }
                        for id in &ids {
                            identifier(&json!(id))?;
                        }
                        (
                            "/api/playlist/list/get",
                            json!({"ids":format!("[{}]", ids.join(",")),"newStyle":true}),
                            Some(ids),
                        )
                    }
                };
                let response = api.call(path, body, false, &[200]).await?;
                let items = response["playlists"]
                    .as_array()
                    .filter(|rows| rows.len() <= LIMIT)
                    .ok_or(Error::Response("invalid category playlists"))?
                    .iter()
                    .map(|row| {
                        let id = identifier(&row["id"])?;
                        let name = row["name"]
                            .as_str()
                            .filter(|name| !name.trim().is_empty())
                            .ok_or(Error::Response("missing category playlist name"))?;
                        Ok(PlaylistTagCard {
                            id,
                            name: name.chars().take(256).collect(),
                            cover_url: mapping::artwork(&row["coverImgUrl"]),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                if !distinct(
                    &items
                        .iter()
                        .map(|item| item.id.as_str())
                        .collect::<Vec<_>>(),
                ) {
                    return Err(Error::Response("duplicate category playlist"));
                }
                let essential = requested
                    .is_none()
                    .then(|| essential_promo(&response["highQuality"]))
                    .flatten();
                let remaining_ids = if let Some(requested) = requested {
                    if items.iter().any(|item| !requested.contains(&item.id)) {
                        return Err(Error::Response("foreign category playlist"));
                    }
                    vec![]
                } else {
                    let ids = response["playlistIds"]
                        .as_array()
                        .filter(|ids| ids.len() <= 8192)
                        .ok_or(Error::Response("invalid category playlist IDs"))?
                        .iter()
                        .map(identifier)
                        .collect::<Result<Vec<_>>>()?;
                    if !distinct(&ids) {
                        return Err(Error::Response("duplicate category playlist ID"));
                    }
                    ids.into_iter().skip(LIMIT).collect()
                };
                Ok(PlaylistTagPage {
                    items,
                    remaining_ids,
                    essential,
                })
            })
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}

fn distinct<T: Eq + std::hash::Hash>(items: &[T]) -> bool {
    items.iter().collect::<HashSet<_>>().len() == items.len()
}

pub(super) fn header_tags(value: &Value) -> Vec<String> {
    let Some(tags) = value.as_array() else {
        return vec![];
    };
    let mut seen = HashSet::new();
    tags.iter()
        .filter_map(Value::as_str)
        .filter(|tag| valid_tag(tag))
        .filter(|tag| seen.insert(*tag))
        .take(3)
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_tags_preserve_official_raw_order_without_fixed_or_unusable_tags() {
        let reply = json!({"data":[{"tag":"华语","displayName":"其他显示"},
            {"tag":"推荐"},{"tag":"摇滚"},{"tag":"华语"},{"tag":"官方"},
            {"tag":"\t"},{"name":"不应按名称猜标签"}]});
        assert_eq!(top_tags(&reply).unwrap(), ["华语", "摇滚"]);
        assert!(top_tags(&json!({"data":{}})).is_err());
    }

    #[test]
    fn catalogue_preserves_group_and_entry_order_and_uses_raw_tag_names() {
        let value = json!({"categories":{"1":"风格","0":"语种","2":"空分组","01":"无效"},
            "sub":[{"category":1,"name":"摇滚","displayName":"摇滚精选"},
                {"category":0,"name":"华语"},
                {"category":1,"name":"摇滚"},
                {"category":1,"name":"民谣","displayName":"\t"},
                {"category":1,"name":"推荐"},{"category":1,"name":"官方"},
                {"category":5,"name":"未知分组"}]});
        assert_eq!(
            json!(catalogue(&value).unwrap()),
            json!([
                {"id":0,"name":"语种","tags":[{"name":"华语","displayName":null}]},
                {"id":1,"name":"风格","tags":[{"name":"摇滚","displayName":"摇滚精选"},{"name":"民谣","displayName":null}]}
            ])
        );
        assert!(catalogue(&json!({"categories": {}})).is_err());
    }
}
