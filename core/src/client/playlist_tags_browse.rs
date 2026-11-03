use super::{MusicClient, dto::AccountData, mapping};
use crate::{Error, Result, protocol::api::identifier};
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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistTagCard {
    pub id: String,
    pub name: String,
    pub cover_url: Option<String>,
}

impl MusicClient {
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
                        if tag.trim().is_empty()
                            || tag.trim() != tag
                            || tag == "推荐"
                            || tag.chars().count() > 80
                            || tag.chars().any(char::is_control)
                        {
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
                            json!({"ids":ids,"newStyle":true}),
                            Some(ids),
                        )
                    }
                };
                let response = api.call(path, body, false, &[200]).await?;
                let mut items = response["playlists"]
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
                let remaining_ids = if let Some(requested) = requested {
                    if items.iter().any(|item| !requested.contains(&item.id)) {
                        return Err(Error::Response("foreign category playlist"));
                    }
                    items.sort_by_key(|item| requested.iter().position(|id| id == &item.id));
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
        .filter(|tag| {
            !tag.is_empty()
                && tag.trim() == *tag
                && *tag != "推荐"
                && tag.chars().count() <= 80
                && !tag.chars().any(char::is_control)
        })
        .filter(|tag| seen.insert(*tag))
        .take(3)
        .map(str::to_owned)
        .collect()
}
