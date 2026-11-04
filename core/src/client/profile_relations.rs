//! The official user-detail guest and own-account relationship read routes.
mod projection;

use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    protocol::api::{Api, identifier},
};
use projection::{ProfileRelationCard, label, project};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};

pub use projection::ProfileRelationCard as ProfileRelationEntry;

const MAX_PAGES: u16 = 100;
const MAX_HANDLES: usize = 128;
const PRIVACY: &str = "/api/user/personal/page/info";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileRelationKind {
    Followers,
    Following,
    FollowingArtists,
    FollowingUsers,
}

impl ProfileRelationKind {
    fn own_scene(self) -> Option<u8> {
        match self {
            Self::Followers => None,
            Self::Following => Some(0),
            Self::FollowingArtists => Some(1),
            Self::FollowingUsers => Some(2),
        }
    }
}

#[derive(Serialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ProfileRelationAccess {
    Allowed { is_self: bool },
    Denied { message: String },
}

#[derive(Serialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ProfileRelationPage {
    Ready {
        items: Vec<ProfileRelationCard>,
        continuation: Option<String>,
        record_count: Option<u64>,
    },
    Denied {
        message: String,
    },
}

#[derive(Clone, Default)]
struct Position {
    page: u16,
    mixed: Vec<Value>,
}

struct Handle {
    epoch: String,
    uid: String,
    kind: ProfileRelationKind,
    key: String,
    position: Position,
}

#[derive(Default)]
pub(super) struct ProfileRelationHost(Mutex<VecDeque<Handle>>);

impl ProfileRelationHost {
    fn position(
        &self,
        epoch: &str,
        uid: &str,
        kind: ProfileRelationKind,
        key: Option<&str>,
    ) -> Result<Position> {
        let Some(key) = key else {
            return Ok(Position::default());
        };
        self.0
            .lock()
            .map_err(|_| Error::State("profile relation handles unavailable"))?
            .iter()
            .find(|item| {
                item.epoch == epoch && item.uid == uid && item.kind == kind && item.key == key
            })
            .map(|item| item.position.clone())
            .ok_or(Error::StaleResponse)
    }

    fn save(
        &self,
        epoch: &str,
        uid: &str,
        kind: ProfileRelationKind,
        position: Position,
    ) -> Result<String> {
        let mut handles = self
            .0
            .lock()
            .map_err(|_| Error::State("profile relation handles unavailable"))?;
        handles.retain(|item| item.epoch == epoch);
        if handles.len() == MAX_HANDLES {
            handles.pop_front();
        }
        let key = uuid::Uuid::new_v4().simple().to_string();
        handles.push_back(Handle {
            epoch: epoch.into(),
            uid: uid.into(),
            kind,
            key: key.clone(),
            position,
        });
        Ok(key)
    }
}

async fn access(api: &dyn Api, uid: &str, is_self: bool) -> Result<ProfileRelationAccess> {
    if is_self {
        return Ok(ProfileRelationAccess::Allowed { is_self });
    }
    let result = api
        .call(PRIVACY, json!({"userId":uid}), false, &[200])
        .await?;
    let relation = &result["data"]["userRelationDTO"];
    match relation["canSeeFansOrFollows"].as_bool() {
        Some(true) => Ok(ProfileRelationAccess::Allowed { is_self }),
        Some(false) => Ok(ProfileRelationAccess::Denied {
            message: label(&relation["fansOrFollowsToastText"], 160)
                .unwrap_or_else(|| "主人关闭了权限".into()),
        }),
        None => Err(Error::Response(
            "profile relation privacy state unavailable",
        )),
    }
}

struct ReadPage {
    items: Vec<ProfileRelationCard>,
    total: Option<u64>,
    next: Option<Position>,
}

fn cursor_key(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control) => {
            Some(s.clone())
        }
        Value::Number(n) if n.is_u64() => Some(n.to_string()),
        _ => None,
    }
}

async fn read(
    api: &dyn Api,
    uid: &str,
    kind: ProfileRelationKind,
    viewer: &str,
    mut pos: Position,
) -> Result<ReadPage> {
    let is_self = uid == viewer;
    let scene = is_self.then_some(kind.own_scene()).flatten();
    if !is_self
        && matches!(
            kind,
            ProfileRelationKind::FollowingArtists | ProfileRelationKind::FollowingUsers
        )
    {
        return Err(Error::InvalidInput(
            "profile relation filter is for the owner",
        ));
    }
    let (path, body, limit) = if let Some(scene) = scene {
        let cursor = pos.mixed.last().cloned().unwrap_or_else(|| json!("0"));
        (
            "/api/user/follow/users/mixed/get/v2".to_owned(),
            json!({"page":{"size":19,"cursor":cursor},"size":19,"scene":scene,"sortType":0,"authority":false}),
            19,
        )
    } else {
        let offset = u32::from(pos.page) * 20;
        match kind {
            ProfileRelationKind::Followers => (
                "/api/user/getfolloweds/".into(),
                json!({"userId":uid,"time":0,"offset":offset,"limit":20,"getcounts":true}),
                20,
            ),
            ProfileRelationKind::Following => (
                format!("/api/user/getfollows/{uid}"),
                json!({"offset":offset,"limit":20,"getcounts":true}),
                20,
            ),
            _ => return Err(Error::InvalidInput("invalid profile relation list")),
        }
    };
    let result = api.call(&path, body, false, &[200]).await?;
    let (rows, page, total) = if scene.is_some() {
        let data = &result["data"];
        (
            &data["records"],
            &data["page"],
            &data["page"]["recordCount"],
        )
    } else if kind == ProfileRelationKind::Followers {
        (&result["followeds"], &result, &result["size"])
    } else {
        (&result["follow"], &result, &result["size"])
    };
    let rows = rows
        .as_array()
        .filter(|rows| rows.len() <= limit)
        .ok_or(Error::Response("invalid profile relation records"))?;
    let more = page["more"]
        .as_bool()
        .ok_or(Error::Response("missing profile relation continuation"))?;
    if more && rows.is_empty() {
        return Err(Error::Response("empty profile relation continuation"));
    }
    if more {
        if scene.is_some() {
            let raw = &page["cursor"];
            let next = cursor_key(raw).ok_or(Error::Response("invalid profile relation cursor"))?;
            if next == "0"
                || pos
                    .mixed
                    .iter()
                    .any(|old| cursor_key(old).as_deref() == Some(next.as_str()))
            {
                return Err(Error::Response("repeated profile relation cursor"));
            }
            pos.mixed.push(raw.clone());
        }
        pos.page += 1;
        if pos.page >= MAX_PAGES {
            return Err(Error::Response("profile relation page limit reached"));
        }
    }
    Ok(ReadPage {
        items: rows
            .iter()
            .filter_map(|row| project(row, scene, viewer))
            .collect(),
        total: total
            .as_u64()
            .filter(|value| *value <= 9_007_199_254_740_991),
        next: more.then_some(pos),
    })
}

impl MusicClient {
    pub async fn profile_relation_access(
        &self,
        epoch: &str,
        uid: &str,
    ) -> Result<AccountData<ProfileRelationAccess>> {
        let token = self.playlist_token(epoch)?;
        let uid = identifier(&json!(uid))?;
        let value = self
            .session
            .run_for(&token, |api, viewer| async move {
                let is_self = uid == viewer;
                access(api.as_ref(), &uid, is_self).await
            })
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub async fn profile_relation_page(
        &self,
        epoch: &str,
        uid: &str,
        kind: ProfileRelationKind,
        continuation: Option<&str>,
    ) -> Result<AccountData<ProfileRelationPage>> {
        let token = self.playlist_token(epoch)?;
        let uid = identifier(&json!(uid))?;
        let pos = self
            .profile_relations
            .position(epoch, &uid, kind, continuation)?;
        let target = uid.clone();
        let value = self
            .session
            .run_for(&token, |api, viewer| async move {
                let is_self = target == viewer;
                // The destination rechecks guest privacy on its first read. Do
                // not issue the list read until positive evidence has arrived.
                if !is_self
                    && matches!(
                        kind,
                        ProfileRelationKind::FollowingArtists | ProfileRelationKind::FollowingUsers
                    )
                {
                    return Err(Error::InvalidInput(
                        "profile relation filter is for the owner",
                    ));
                }
                if continuation.is_none()
                    && let ProfileRelationAccess::Denied { message } =
                        access(api.as_ref(), &target, is_self).await?
                {
                    return Ok(Err(message));
                }
                Ok(Ok(read(api.as_ref(), &target, kind, &viewer, pos).await?))
            })
            .await?;
        self.session.with_token(&token, |_| {
            let value = match value {
                Err(message) => ProfileRelationPage::Denied { message },
                Ok(page) => ProfileRelationPage::Ready {
                    items: page.items,
                    record_count: page.total,
                    continuation: page
                        .next
                        .map(|pos| self.profile_relations.save(epoch, &uid, kind, pos))
                        .transpose()?,
                },
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
