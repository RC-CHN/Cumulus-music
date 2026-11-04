use super::{
    MusicClient,
    dto::{AccountData, PlaylistView},
    mapping,
    podcast::profile::{ProfilePodcastEntry, profile_podcast_count},
};
use crate::{Error, Result, protocol::api::identifier};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

const LIST_LIMIT: usize = 1000;
const MAX_PAGES: usize = 8;

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
    pub playlists_visible: bool,
    pub follow: Option<super::profile_follow::ProfileFollowView>,
    pub blacklist: Option<super::profile_blacklist::ProfileBlacklistHeader>,
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
                let (mut created, mut collected) = (Vec::new(), Vec::new());
                if visible {
                    let mut pages = Vec::new();
                    let mut more = true;
                    for page in 0..MAX_PAGES {
                        self.session.require_api(request_token, &api)?;
                        let offset = page * LIST_LIMIT;
                        let mut result = api
                            .call(
                                "/api/user/playlist",
                                json!({"uid":uid,"offset":offset,"limit":LIST_LIMIT}),
                                false,
                                &[200],
                            )
                            .await?;
                        let rows = std::mem::take(&mut result["playlist"]);
                        let Value::Array(rows) = rows else {
                            return Err(Error::Response("invalid profile playlist page"));
                        };
                        if rows.len() > LIST_LIMIT {
                            return Err(Error::Response("invalid profile playlist page"));
                        }
                        more = result["more"]
                            .as_bool()
                            .ok_or(Error::Response("missing profile playlist pagination"))?;
                        if more && rows.is_empty() {
                            return Err(Error::Response("empty profile playlist continuation"));
                        }
                        if !rows.is_empty() {
                            self.session.require_api(request_token, &api)?;
                            // M/232 orders each base page before separating the two groups.
                            // An optional pin request must not erase the authoritative base page.
                            let pins = api
                                .call(
                                    "/api/user/playlist/getTop",
                                    json!({"userId":uid,"offset":offset,"limit":LIST_LIMIT}),
                                    false,
                                    &[200],
                                )
                                .await
                                .ok()
                                .and_then(|mut response| {
                                    match response
                                        .get_mut("data")
                                        .and_then(|data| data.get_mut("topItemList"))
                                        .map(std::mem::take)
                                    {
                                        Some(Value::Array(rows)) if rows.len() <= LIST_LIMIT => {
                                            Some(rows)
                                        }
                                        _ => None,
                                    }
                                })
                                .unwrap_or_default();
                            pages.push((rows, pins));
                        }
                        if !more {
                            break;
                        }
                    }
                    if more {
                        return Err(Error::Response("profile playlist pagination limit reached"));
                    }
                    self.session.require_api(request_token, &api)?;
                    (created, collected) = project_profile_playlists(&pages, &viewer, &uid);
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
                    playlists_visible: visible,
                    follow: self.profile_follow_header(request_token, &viewer, &uid, profile)?,
                    blacklist: self.profile_blacklist_header(
                        request_token,
                        &viewer,
                        &uid,
                        profile,
                    )?,
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

/// Match M/232.q's first-row / pinned / remaining order while retaining the
/// base response's ownership/privacy when the pin references a known playlist.
fn project_profile_playlists(
    pages: &[(Vec<Value>, Vec<Value>)],
    viewer: &str,
    uid: &str,
) -> (Vec<PlaylistView>, Vec<PlaylistView>) {
    let mut base = HashMap::<String, &Value>::new();
    let mut conflicts = HashSet::new();
    for (rows, _) in pages {
        for row in rows {
            if let Ok(id) = identifier(&row["id"]) {
                if let Some(previous) = base.get(&id) {
                    if previous["privacy"] != row["privacy"]
                        || playlist_owner(previous) != playlist_owner(row)
                    {
                        conflicts.insert(id);
                    }
                } else {
                    base.insert(id, row);
                }
            }
        }
    }
    let mut seen = HashSet::new();
    let (mut created, mut collected) = (Vec::new(), Vec::new());
    let mut push = |row: &Value| {
        let Ok(id) = identifier(&row["id"]) else {
            return;
        };
        if conflicts.contains(&id) {
            return;
        }
        let Some((owner, declared)) = playlist_owner(row) else {
            return;
        };
        let own = owner == uid;
        match row["privacy"].as_i64() {
            Some(0) => (),
            // Private playlists must explicitly name the authenticated owner.
            Some(10) if own && viewer == uid && declared => (),
            _ => return,
        }
        if let Ok(card) = mapping::playlist(row, viewer)
            && seen.insert(card.id.clone())
        {
            if own {
                created.push(card)
            } else {
                collected.push(card)
            }
        }
    };

    for (rows, pins) in pages {
        let Some((first, rest)) = rows.split_first() else {
            continue;
        };
        push(first);
        for pin in pins {
            let Ok(id) = identifier(&pin["id"]) else {
                continue;
            };
            let row = if let Some(row) = base.get(&id) {
                // Pins can be sparse. Use authoritative base metadata even
                // when the corresponding row appeared on another base page.
                if (!pin["privacy"].is_null() && pin["privacy"] != row["privacy"])
                    || (!pin["userId"].is_null() || !pin["creator"]["userId"].is_null())
                        && playlist_owner(pin).map(|value| value.0)
                            != playlist_owner(row).map(|value| value.0)
                {
                    continue;
                }
                *row
            } else {
                // A pin can appear outside the ordinary pages. A private item
                // needs authoritative base membership before disclosing it.
                if pin["privacy"].as_i64() != Some(0) {
                    continue;
                }
                pin
            };
            push(row);
        }
        for row in rest {
            push(row);
        }
    }
    (created, collected)
}

fn playlist_owner(row: &Value) -> Option<(String, bool)> {
    let direct = identifier(&row["userId"]).ok();
    let creator = identifier(&row["creator"]["userId"]).ok();
    if matches!((&direct, &creator), (Some(a), Some(b)) if a != b) {
        return None;
    }
    let declared = direct.is_some();
    Some((direct.or(creator)?, declared))
}

fn relation_count(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .filter(|value| *value <= 9_007_199_254_740_991)
}
