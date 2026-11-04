//! Recommendation tab of the official playlist square (M/2306).
use super::{MusicClient, dto::AccountData, mapping};
use crate::{Error, Result, account::AccountApi, protocol::api::identifier};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{HashSet, VecDeque},
    sync::Mutex,
};

#[derive(Default)]
pub(super) struct PlaylistSquareHost(Mutex<VecDeque<(String, String, String)>>);

impl PlaylistSquareHost {
    fn resolve(&self, epoch: &str, handle: Option<&str>) -> Result<String> {
        let Some(handle) = handle else {
            return Ok(String::new());
        };
        self.0
            .lock()
            .map_err(|_| Error::State("playlist square cursors unavailable"))?
            .iter()
            .find(|(owner, key, _)| owner == epoch && key == handle)
            .map(|(_, _, cursor)| cursor.clone())
            .ok_or(Error::StaleResponse)
    }

    fn save(&self, epoch: &str, cursor: String) -> Result<String> {
        let mut rows = self
            .0
            .lock()
            .map_err(|_| Error::State("playlist square cursors unavailable"))?;
        rows.retain(|(owner, _, _)| owner == epoch);
        if let Some((_, key, _)) = rows.iter().find(|(_, _, saved)| *saved == cursor) {
            return Ok(key.clone());
        }
        if rows.len() == 64 {
            rows.pop_front();
        }
        let key = uuid::Uuid::new_v4().simple().to_string();
        rows.push_back((epoch.into(), key.clone(), cursor));
        Ok(key)
    }

    fn forget(&self, epoch: &str) -> Result<()> {
        self.0
            .lock()
            .map_err(|_| Error::State("playlist square cursors unavailable"))?
            .retain(|(owner, _, _)| owner != epoch);
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSquarePage {
    recommendation_enabled: bool,
    groups: Vec<Group>,
    continuation: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Group {
    title: Option<String>,
    items: Vec<Card>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Card {
    id: String,
    name: String,
    cover_url: Option<String>,
    play_count: Option<u64>,
}

fn label(value: &Value) -> Option<String> {
    let value = value.as_str()?.trim();
    (!value.is_empty() && value.chars().count() <= 200 && !value.chars().any(char::is_control))
        .then(|| value.into())
}

fn project(data: &Value) -> Result<(Vec<Group>, Option<String>)> {
    let rows = data["blocks"]
        .as_array()
        .filter(|rows| rows.len() <= 48)
        .ok_or(Error::Response("invalid playlist square blocks"))?;
    let has_more = data["hasMore"].as_bool().ok_or(Error::Response(
        "missing playlist square continuation state",
    ))?;
    let cursor = if has_more {
        Some(
            data["cursor"]
                .as_str()
                .filter(|cursor| !cursor.is_empty() && cursor.len() <= 4096)
                .ok_or(Error::Response("invalid playlist square continuation"))?
                .to_owned(),
        )
    } else {
        None
    };
    let mut groups = Vec::new();
    for row in rows.iter().filter(|row| {
        matches!(
            row["showType"].as_str(),
            Some("PLAYLIST_SQUARE_MULTI_THREE_GRID" | "PLAYLIST_SQUARE_SINGLE_SLID")
        )
    }) {
        let creatives = row["creatives"]
            .as_array()
            .filter(|creatives| creatives.len() <= 48)
            .ok_or(Error::Response("invalid playlist square creatives"))?;
        let mut seen = HashSet::new();
        let mut items = Vec::new();
        for creative in creatives
            .iter()
            .filter(|creative| creative["creativeType"] == "list")
        {
            let resources = creative["resources"]
                .as_array()
                .filter(|resources| resources.len() <= 64)
                .ok_or(Error::Response("invalid playlist square resources"))?;
            for resource in resources
                .iter()
                .filter(|resource| resource["resourceType"] == "playlist")
            {
                let Ok(id) = identifier(&resource["resourceId"]) else {
                    continue;
                };
                let Some(name) = label(&resource["uiElement"]["mainTitle"]["title"]) else {
                    continue;
                };
                if !seen.insert(id.clone()) {
                    continue;
                }
                items.push(Card {
                    id,
                    name,
                    cover_url: mapping::artwork(&resource["uiElement"]["images"][0]["imageUrl"]),
                    play_count: resource["resourceExt"]["playCount"]
                        .as_u64()
                        .filter(|count| *count <= 9_007_199_254_740_991),
                });
                if items.len() > 64 {
                    return Err(Error::Response("too many playlist square cards"));
                }
            }
        }
        if !items.is_empty() {
            groups.push(Group {
                title: label(&row["uiElement"]["mainTitle"]["title"]),
                items,
            });
        }
    }
    if groups.iter().map(|group| group.items.len()).sum::<usize>() > 192 {
        return Err(Error::Response("too many playlist square cards per page"));
    }
    Ok((groups, cursor))
}

impl MusicClient {
    pub async fn playlist_square_recommendations(
        &self,
        epoch: &str,
        continuation: Option<&str>,
    ) -> Result<AccountData<PlaylistSquarePage>> {
        let token = self.playlist_token(epoch)?;
        let cursor = self.playlist_square.resolve(epoch, continuation)?;
        let reply = self
            .session
            .run_for(&token, |api, _| async move {
                // Respect the application's existing personalized-feed setting before
                // every request, including reuse of a previously issued continuation.
                if AccountApi::new(api.clone()).settings().await?["personalServiceSetting"] == 2 {
                    return Ok(None);
                }
                let response = api
                    .call(
                        "/api/playlist/square/block/page",
                        json!({
                            "cursor":cursor, "newStyle":true,
                        }),
                        false,
                        &[200],
                    )
                    .await?;
                project(&response["data"]).map(Some)
            })
            .await?;
        self.session.with_token(&token, |_| {
            let value = match reply {
                None => {
                    self.playlist_square.forget(epoch)?;
                    PlaylistSquarePage {
                        recommendation_enabled: false,
                        groups: Vec::new(),
                        continuation: None,
                    }
                }
                Some((groups, cursor)) => {
                    let continuation = cursor
                        .map(|cursor| self.playlist_square.save(epoch, cursor))
                        .transpose()?;
                    PlaylistSquarePage {
                        recommendation_enabled: true,
                        groups,
                        continuation,
                    }
                }
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_square_projects_only_known_blocks_and_scopes_opaque_cursors() {
        let card = json!({"resourceType":"playlist","resourceId":21,
            "uiElement":{"mainTitle":{"title":"合成歌单"},"images":[{"imageUrl":"https://p1.music.126.net/a.jpg?secret=private"}]},
            "resourceExt":{"playCount":12},"action":{"url":"unsafe"}});
        let reply = json!({"hasMore":true,"cursor":"{ \"offset\" : 2 }","blocks":[
            {"showType":"promotion","creatives":[]},
            {"showType":"PLAYLIST_SQUARE_SINGLE_SLID","uiElement":{"mainTitle":{"title":"分组"}},
             "creatives":[{"creativeType":"other","resources":[card.clone()]},
               {"creativeType":"list","resources":[{"resourceType":"video","resourceId":22},card.clone(),card]}]}
        ]});
        let (groups, raw) = project(&reply).unwrap();
        assert_eq!(
            json!(groups),
            json!([{"title":"分组","items":[{"id":"21","name":"合成歌单",
                "coverUrl":"https://p1.music.126.net/a.jpg?param=320y320","playCount":12}]}])
        );
        let host = PlaylistSquareHost::default();
        let raw = raw.unwrap();
        let token = host.save("one", raw.clone()).unwrap();
        assert_ne!(token, raw);
        assert_eq!(host.resolve("one", Some(&token)).unwrap(), raw);
        assert!(host.resolve("two", Some(&token)).is_err());
        host.forget("one").unwrap();
        assert!(host.resolve("one", Some(&token)).is_err());
        assert!(project(&json!({"blocks":[],"hasMore":true})).is_err());
    }
}
