//! Profile audio: M/2264 create/favourite effects, M/232.n and M/15 Lb/Mb/Nb.
use super::{MusicClient, PodcastCard, view};
use crate::{
    Error, Result,
    account::SessionToken,
    client::dto::AccountData,
    error::code_matches,
    podcast::Radio,
    protocol::{
        api::{Api, identifier},
        wire::serialization::encode_component,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};

const PAGE_SIZE: u64 = 20;
const MAX_CREATED_OFFSET: u64 = 1_000_000;
const MAX_COLLECTED: u64 = 1000;
const MAX_SAFE_COUNT: u64 = 9_007_199_254_740_991;
const MANAGEMENT_ROUTE: &str = "/radio/my/#/plist";
const MANAGEMENT_URL: &str = "https://music.163.com/radio/my/#/plist";

/// Short-lived native browser handoff; deliberately neither serializable nor debuggable.
pub struct ProfilePodcastManagement {
    token: SessionToken,
    target: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfilePodcastQuery {
    Created { offset: u64 },
    Collected,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilePodcastEntry {
    pub is_self: bool,
    pub total: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilePodcastPage {
    pub rows: Vec<ProfilePodcastRow>,
    pub total: u64,
    pub offset: u64,
    pub next_offset: Option<u64>,
    pub truncated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilePodcastRow {
    pub card: PodcastCard,
    pub subscribers: u64,
}

fn safe_count(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|value| *value <= MAX_SAFE_COUNT)
}

pub(in crate::client) async fn profile_podcast_count(api: &dyn Api, uid: &str) -> u64 {
    // The official header renders zero when this independent request fails.
    let Ok(result) = api
        .call(
            "/api/djradio/get/create/sub/count",
            json!({"userId":uid}),
            false,
            &[200],
        )
        .await
    else {
        return 0;
    };
    (safe_count(&result["createRadioCount"]).unwrap_or(0)
        + safe_count(&result["subRadioCount"]).unwrap_or(0))
    .min(MAX_SAFE_COUNT)
}

fn inaccessible(error: &Error) -> bool {
    error.is_service_code(403)
        || error.is_service_code(404)
        || matches!(
            error.root(),
            Error::HttpStatus {
                status: 403 | 404,
                ..
            }
        )
}

impl MusicClient {
    async fn profile_podcast_access(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        viewer: &str,
        uid: &str,
    ) -> Result<bool> {
        if viewer == uid {
            return Ok(true);
        }
        let detail = match api
            .call(
                &format!("/api/w/v1/user/detail/{uid}"),
                json!({"all":true,"userId":uid}),
                false,
                &[200, 403, 404],
            )
            .await
        {
            Ok(result) if code_matches(&result["code"], 200) => result,
            Ok(_) => return Ok(false),
            Err(error) if inaccessible(&error) => return Ok(false),
            Err(error) => return Err(error),
        };
        let profile = &detail["profile"];
        if identifier(&profile["userId"]).ok().as_deref() != Some(uid) {
            return Err(Error::Response("podcast profile identity mismatch"));
        }
        self.session.require_api(token, api)?;
        Ok(profile["inBlacklist"].as_bool() != Some(true))
    }

    pub async fn profile_podcasts(
        &self,
        epoch: &str,
        uid: &str,
        query: ProfilePodcastQuery,
    ) -> Result<AccountData<Option<ProfilePodcastPage>>> {
        let token = self.podcast_token(epoch)?;
        let uid = identifier(&json!(uid))?;
        let (path, body, offset, limit) = match query {
            ProfilePodcastQuery::Created { offset }
                if offset <= MAX_CREATED_OFFSET && offset.is_multiple_of(PAGE_SIZE) =>
            {
                (
                    "/api/djradio/get/byuser/v1",
                    json!({"userId":uid,"offset":offset,"limit":PAGE_SIZE}),
                    offset,
                    PAGE_SIZE,
                )
            }
            ProfilePodcastQuery::Created { .. } => {
                return Err(Error::InvalidInput("invalid profile podcast offset"));
            }
            ProfilePodcastQuery::Collected => (
                "/api/djradio/get/subed",
                json!({"targetUserId":uid,"limit":MAX_COLLECTED}),
                0,
                MAX_COLLECTED,
            ),
        };
        let value = self
            .session
            .run_for(&token, |api, viewer| {
                let token = &token;
                async move {
                    if !self
                        .profile_podcast_access(token, &api, &viewer, &uid)
                        .await?
                    {
                        return Ok(None);
                    }
                    let response = match api.call(path, body, false, &[200, 403, 404]).await {
                        Ok(result) if code_matches(&result["code"], 200) => result,
                        Ok(_) => return Ok(None),
                        Err(error) if inaccessible(&error) => return Ok(None),
                        Err(error) => return Err(error),
                    };
                    project(response, &uid, query, offset, limit).map(Some)
                }
            })
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub async fn prepare_profile_podcast_management(
        &self,
        epoch: &str,
        uid: &str,
    ) -> Result<ProfilePodcastManagement> {
        let token = self.podcast_token(epoch)?;
        let uid = identifier(&json!(uid))?;
        let target = self
            .session
            .run_for(&token, |api, viewer| async move {
                if uid != viewer {
                    return Err(Error::State("only the owner can manage their podcasts"));
                }
                // M/73 ignores business codes and falls back to the plain URL
                // if the response provides no short-lived browser token.
                let response = api.request("/api/sso/login/token", json!({}), false).await.ok();
                let browser_token = response.as_ref().and_then(|v| v["token"].as_str()).filter(|v| {
                    !v.trim().is_empty() && v.len() <= 4096 && !v.chars().any(char::is_control)
                });
                Ok(match browser_token {
                    Some(key) => format!(
                        "https://music.163.com/sso/login?token={}&uid={}&url={}&forceReplaceLogin=true",
                        encode_component(key), encode_component(&viewer), encode_component(MANAGEMENT_ROUTE),
                    ),
                    None => MANAGEMENT_URL.into(),
                })
            })
            .await?;
        Ok(ProfilePodcastManagement { token, target })
    }

    pub fn launch_profile_podcast_management<T>(
        &self,
        handoff: ProfilePodcastManagement,
        launch: impl FnOnce(&str) -> Result<T>,
    ) -> Result<T> {
        self.session.with_token(&handoff.token, |_| Ok(()))?;
        launch(&handoff.target)
    }
}

fn project(
    response: Value,
    uid: &str,
    query: ProfilePodcastQuery,
    offset: u64,
    limit: u64,
) -> Result<ProfilePodcastPage> {
    let raw = response["djRadios"]
        .as_array()
        .filter(|rows| rows.len() as u64 <= limit)
        .ok_or(Error::Response("invalid profile podcast rows"))?;
    let mut seen = HashSet::new();
    let rows = raw
        .iter()
        .filter_map(|item| {
            if matches!(query, ProfilePodcastQuery::Created { .. }) {
                let creator = item["dj"]["userId"]
                    .as_str()
                    .or_else(|| item["creator"]["userId"].as_str())
                    .map(str::to_owned)
                    .or_else(|| identifier(&item["dj"]["userId"]).ok())
                    .or_else(|| identifier(&item["creator"]["userId"]).ok());
                if creator.as_deref().is_some_and(|creator| creator != uid) {
                    return None;
                }
            }
            let radio = Radio::from_official(item).ok()?;
            if radio.name.trim().is_empty() || !seen.insert(radio.id.clone()) {
                return None;
            }
            let mut card = view::card(&radio);
            // A profile's list is not the current listener's subscription state.
            card.subscribed = None;
            Some(ProfilePodcastRow {
                card,
                subscribers: safe_count(&item["subCount"]).unwrap_or(0),
            })
        })
        .collect::<Vec<_>>();
    let remote = safe_count(&response["count"]);
    let (total, next_offset, truncated) = match query {
        ProfilePodcastQuery::Created { .. } => {
            let total = remote.ok_or(Error::Response("missing profile created podcast count"))?;
            let next = offset + limit;
            (
                total,
                (next < total && next <= MAX_CREATED_OFFSET).then_some(next),
                next < total && next > MAX_CREATED_OFFSET,
            )
        }
        ProfilePodcastQuery::Collected => {
            let raw_count = raw.len() as u64;
            let total = remote.unwrap_or(raw_count).max(raw_count);
            let truncated = response["hasMore"] == true
                || total > raw_count
                || (raw_count == limit && response["hasMore"] != false);
            (total, None, truncated)
        }
    };
    Ok(ProfilePodcastPage {
        rows,
        total,
        offset,
        next_offset,
        truncated,
    })
}
