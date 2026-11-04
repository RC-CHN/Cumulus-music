//! High-quality playlist category route recovered from M/3148.
use super::{MusicClient, dto::AccountData, mapping, playlist_tags_browse::safe_label};
use crate::{Error, Result, protocol::api::identifier};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{HashSet, VecDeque},
    sync::Mutex,
};

const LIMIT: usize = 36;

struct Cursor {
    epoch: String,
    category: String,
    handle: String,
    lasttime: Value,
}

#[derive(Default)]
pub(super) struct PlaylistEssentialHost(Mutex<VecDeque<Cursor>>);

impl PlaylistEssentialHost {
    fn resolve(&self, epoch: &str, category: &str, handle: Option<&str>) -> Result<Value> {
        let Some(handle) = handle else {
            return Ok(json!("-1"));
        };
        self.0
            .lock()
            .map_err(|_| Error::State("playlist essential cursors unavailable"))?
            .iter()
            .find(|entry| {
                entry.epoch == epoch && entry.category == category && entry.handle == handle
            })
            .map(|entry| entry.lasttime.clone())
            .ok_or(Error::StaleResponse)
    }

    fn save(&self, epoch: &str, category: &str, lasttime: Value) -> Result<String> {
        let mut entries = self
            .0
            .lock()
            .map_err(|_| Error::State("playlist essential cursors unavailable"))?;
        entries.retain(|entry| entry.epoch == epoch);
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.category == category && entry.lasttime == lasttime)
        {
            return Ok(entry.handle.clone());
        }
        if entries.len() == 64 {
            entries.pop_front();
        }
        let handle = uuid::Uuid::new_v4().simple().to_string();
        entries.push_back(Cursor {
            epoch: epoch.into(),
            category: category.into(),
            handle: handle.clone(),
            lasttime,
        });
        Ok(handle)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEssentialCategory {
    name: String,
    display_name: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEssentialPage {
    items: Vec<PlaylistEssentialCard>,
    continuation: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistEssentialCard {
    id: String,
    name: String,
    cover_url: Option<String>,
    play_count: Option<u64>,
}

fn categories(reply: &Value) -> Result<Vec<PlaylistEssentialCategory>> {
    let tags = reply["tags"]
        .as_array()
        .filter(|rows| rows.len() <= 256)
        .ok_or(Error::Response("invalid essential playlist tags"))?;
    let mut result = vec![PlaylistEssentialCategory {
        name: "全部".into(),
        display_name: Some("全部分类".into()),
    }];
    let mut seen = HashSet::from(["全部"]);
    result.extend(tags.iter().filter_map(|row| {
        let name = row["name"]
            .as_str()
            .filter(|name| safe_label(name) && seen.insert(*name))?;
        let display_name = row["displayName"]
            .as_str()
            .filter(|value| safe_label(value))
            .map(str::to_owned);
        Some(PlaylistEssentialCategory {
            name: name.into(),
            display_name,
        })
    }));
    Ok(result)
}

fn project(reply: &Value) -> Result<(Vec<PlaylistEssentialCard>, Option<Value>)> {
    let rows = reply["playlists"]
        .as_array()
        .filter(|rows| rows.len() <= LIMIT)
        .ok_or(Error::Response("invalid essential playlist page"))?;
    let more = reply["more"].as_bool().ok_or(Error::Response(
        "missing essential playlist continuation state",
    ))?;
    let next = &reply["lasttime"];
    let next = if more {
        if !(next
            .as_str()
            .is_some_and(|value| !value.is_empty() && value.len() <= 512)
            || next.is_i64()
            || next.is_u64())
            || next == "-1"
            || next == -1
        {
            return Err(Error::Response("invalid essential playlist continuation"));
        }
        Some(next.clone())
    } else {
        None
    };
    let mut seen = HashSet::new();
    let items = rows
        .iter()
        .filter_map(|row| {
            let id = identifier(&row["id"]).ok()?;
            let name = [row["name"].as_str(), row["title"].as_str()]
                .into_iter()
                .flatten()
                .find(|name| !name.is_empty())?
                .trim();
            if name.is_empty()
                || name.chars().count() > 200
                || name.chars().any(char::is_control)
                || !seen.insert(id.clone())
            {
                return None;
            }
            Some(PlaylistEssentialCard {
                id,
                name: name.into(),
                cover_url: ["coverImgUrl", "picUrl", "coverUrl"]
                    .into_iter()
                    .find(|key| row[*key].as_str().is_some_and(|url| !url.is_empty()))
                    .and_then(|key| mapping::artwork(&row[key])),
                play_count: row["playCount"]
                    .as_u64()
                    .filter(|count| *count <= 9_007_199_254_740_991),
            })
        })
        .collect();
    Ok((items, next))
}

impl MusicClient {
    pub async fn playlist_essential_categories(
        &self,
        epoch: &str,
    ) -> Result<AccountData<Vec<PlaylistEssentialCategory>>> {
        let token = self.playlist_token(epoch)?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                let reply = api
                    .call("/api/playlist/highquality/tags", json!({}), false, &[200])
                    .await?;
                categories(&reply)
            })
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub async fn playlist_essential_page(
        &self,
        epoch: &str,
        category: &str,
        continuation: Option<&str>,
    ) -> Result<AccountData<PlaylistEssentialPage>> {
        let token = self.playlist_token(epoch)?;
        if !safe_label(category) {
            return Err(Error::InvalidInput("invalid essential playlist category"));
        }
        let lasttime = self
            .playlist_essential
            .resolve(epoch, category, continuation)?;
        let body = json!({"cat":category,"limit":LIMIT,"lasttime":lasttime});
        let (items, next) = self
            .session
            .run_for(&token, |api, _| async move {
                let reply = api
                    .call("/api/playlist/highquality/list", body, false, &[200])
                    .await?;
                project(&reply)
            })
            .await?;
        self.session.with_token(&token, |_| {
            let continuation = next
                .map(|next| self.playlist_essential.save(epoch, category, next))
                .transpose()?;
            Ok(AccountData {
                epoch: epoch.into(),
                value: PlaylistEssentialPage {
                    items,
                    continuation,
                },
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_high_quality_cursor_preserves_json_type_and_category() {
        let tags = categories(
            &json!({"tags":[{"name":"轻音乐","displayName":"放松"},{"name":"全部"},{"name":"\t"}]}),
        )
        .unwrap();
        assert_eq!(
            json!(tags),
            json!([{"name":"全部","displayName":"全部分类"},{"name":"轻音乐","displayName":"放松"}])
        );
        let host = PlaylistEssentialHost::default();
        assert_eq!(host.resolve("one", "轻音乐", None).unwrap(), json!("-1"));
        let cursor = 1_700_000_000_123_u64;
        let (_, next) = project(&json!({"more":true,"lasttime":cursor,"playlists":[]})).unwrap();
        let handle = host.save("one", "轻音乐", next.unwrap()).unwrap();
        assert_eq!(
            host.resolve("one", "轻音乐", Some(&handle)).unwrap(),
            json!(cursor)
        );
        assert!(host.resolve("one", "全部", Some(&handle)).is_err());
        assert!(host.resolve("two", "轻音乐", Some(&handle)).is_err());
        let (_, next) = project(&json!({"more":true,"lasttime":"00123","playlists":[]})).unwrap();
        assert_eq!(next, Some(json!("00123")));
        assert!(project(&json!({"more":true,"lasttime":"-1","playlists":[]})).is_err());
        let (items, _) = project(&json!({"more":false,"playlists":[{
            "id":4,"name":"","title":"合成歌单","picUrl":"https://p1.music.126.net/a.jpg?secret=private"
        }]})).unwrap();
        assert_eq!(
            json!(items),
            json!([{"id":"4","name":"合成歌单",
            "coverUrl":"https://p1.music.126.net/a.jpg?param=320y320","playCount":null}])
        );
    }
}
