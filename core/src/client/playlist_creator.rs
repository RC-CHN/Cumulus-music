use super::{
    MusicClient,
    dto::{AccountData, PlaylistView},
    mapping,
    podcast::profile::{ProfilePodcastEntry, profile_podcast_count},
};
use crate::{Error, Result, protocol::api::identifier};
use serde::Serialize;
use serde_json::json;
use std::collections::HashSet;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistCreatorProfile {
    pub id: String,
    pub nickname: String,
    pub avatar_url: Option<String>,
    pub signature: String,
    pub following_count: Option<u64>,
    pub followers_count: Option<u64>,
    pub notes_count: Option<u64>,
    pub podcasts: Option<ProfilePodcastEntry>,
    pub listening_rank: Option<ListeningRankEntry>,
    pub created: Vec<PlaylistView>,
    pub collected: Vec<PlaylistView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListeningRankEntry {
    pub is_self: bool,
    pub listen_songs: Option<u64>,
}

impl MusicClient {
    pub async fn playlist_creator_profile(
        &self,
        epoch: &str,
        uid: &str,
    ) -> Result<AccountData<Option<PlaylistCreatorProfile>>> {
        let token = self.playlist_token(epoch)?;
        let uid = identifier(&json!(uid))?;
        let request_token = &token;
        let value = self
            .session
            .run_for(&token, |api, viewer| async move {
                let path = format!("/api/w/v1/user/detail/{uid}");
                let response = match api
                    .call(&path, json!({"all":true,"userId":uid}), false, &[200, 404])
                    .await
                {
                    Ok(response) if response["code"] == 404 || response["code"] == "404" => {
                        return Ok(None);
                    }
                    Err(Error::HttpStatus { status: 404, .. }) => return Ok(None),
                    other => other?,
                };
                let profile = &response["profile"];
                if identifier(&profile["userId"]).ok().as_deref() != Some(uid.as_str()) {
                    return Err(Error::Response("profile identity mismatch"));
                }
                let nickname = ["nickname", "nickName", "userName"]
                    .iter()
                    .filter_map(|key| profile[*key].as_str())
                    .find(|name| !name.trim().is_empty())
                    .ok_or(Error::Response("missing profile nickname"))?;
                let visible = viewer == uid || profile["inBlacklist"].as_bool() != Some(true);
                let podcasts = if visible {
                    self.session.require_api(request_token, &api)?;
                    Some(ProfilePodcastEntry {
                        is_self: viewer == uid,
                        total: profile_podcast_count(api.as_ref(), &uid).await,
                    })
                } else {
                    None
                };
                self.session.require_api(request_token, &api)?;
                let (mut created, mut collected) = (Vec::new(), Vec::new());
                let mut seen = HashSet::new();
                let mut more = true;
                const LIMIT: usize = 1000;
                for page in 0..8 {
                    let result = api
                        .call(
                            "/api/user/playlist",
                            json!({"uid":uid,"offset":page*LIMIT,"limit":LIMIT}),
                            false,
                            &[200],
                        )
                        .await?;
                    let rows = result["playlist"]
                        .as_array()
                        .filter(|rows| rows.len() <= LIMIT)
                        .ok_or(Error::Response("invalid public playlist page"))?;
                    more = result["more"]
                        .as_bool()
                        .ok_or(Error::Response("missing public playlist pagination"))?;
                    if more && rows.is_empty() {
                        return Err(Error::Response("empty public playlist continuation"));
                    }
                    for row in rows {
                        // This destination is public even when the visitor owns the profile.
                        if row["privacy"].as_i64() != Some(0) {
                            continue;
                        }
                        let owner = identifier(&row["userId"])
                            .or_else(|_| identifier(&row["creator"]["userId"]))?;
                        let item = mapping::playlist(row, &viewer)?;
                        if !seen.insert(item.id.clone()) {
                            continue;
                        }
                        if owner == uid {
                            created.push(item);
                        } else {
                            collected.push(item);
                        }
                    }
                    if !more {
                        break;
                    }
                }
                if more {
                    return Err(Error::Response("public playlist pagination limit reached"));
                }
                Ok(Some(PlaylistCreatorProfile {
                    id: uid.clone(),
                    nickname: nickname.chars().take(128).collect(),
                    avatar_url: mapping::artwork(&profile["avatarUrl"]),
                    signature: profile["signature"]
                        .as_str()
                        .unwrap_or_default()
                        .chars()
                        .take(512)
                        .collect(),
                    following_count: relation_count(&profile["follows"]),
                    followers_count: relation_count(&profile["followeds"]),
                    notes_count: visible
                        .then(|| relation_count(&profile["eventCount"]).unwrap_or_default()),
                    podcasts,
                    listening_rank: super::profile_play_record::allowed(&response, viewer == uid)
                        .then(|| ListeningRankEntry {
                            is_self: viewer == uid,
                            listen_songs: super::profile_play_record::count(
                                &response["listenSongs"],
                            ),
                        }),
                    created,
                    collected,
                }))
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

fn relation_count(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .filter(|value| *value <= 9_007_199_254_740_991)
}
