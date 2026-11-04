use super::super::mapping;
use crate::protocol::api::identifier;
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRelationCard {
    pub id: String,
    pub kind: &'static str,
    pub name: String,
    pub remark: Option<String>,
    pub avatar_url: Option<String>,
    pub signature: Option<String>,
    pub relationship: Option<&'static str>,
}

pub(super) fn label(value: &Value, max: usize) -> Option<String> {
    let value = value.as_str()?.trim();
    if value.is_empty() || value.chars().count() > max || value.chars().any(char::is_control) {
        return None;
    }
    Some(value.to_owned())
}

fn user(raw: &Value, own_mixed: bool, viewer: &str) -> Option<ProfileRelationCard> {
    let id = identifier(&raw["userId"]).ok()?;
    let nickname = label(&raw["nickname"], 128);
    let remark = label(&raw["remarkName"], 128);
    let name = nickname.or_else(|| remark.clone())?;
    let follows = id != viewer && (own_mixed || raw["followed"] == true);
    let mutual = follows && raw["mutual"] == true;
    Some(ProfileRelationCard {
        id,
        kind: "user",
        remark: remark.filter(|value| *value != name),
        name,
        avatar_url: mapping::artwork(&raw["avatarUrl"]),
        signature: label(&raw["signature"], 512),
        relationship: if mutual {
            Some("mutual")
        } else if follows {
            Some("following")
        } else {
            None
        },
    })
}

fn artist(raw: &Value, associated: &Value) -> Option<ProfileRelationCard> {
    let id = identifier(&raw["id"]).ok()?;
    let name = label(&raw["name"], 128)?;
    let avatar_url = if associated.is_object() {
        mapping::artwork(&associated["avatarUrl"])
    } else {
        mapping::artwork(&raw["picUrl"]).or_else(|| mapping::artwork(&raw["img1v1Url"]))
    };
    Some(ProfileRelationCard {
        id,
        kind: "artist",
        name,
        remark: None,
        avatar_url,
        signature: label(&associated["signature"], 512),
        relationship: Some("following"),
    })
}

pub(super) fn project(raw: &Value, scene: Option<u8>, viewer: &str) -> Option<ProfileRelationCard> {
    match scene {
        None => user(raw, false, viewer),
        Some(2) => user(&raw["userProfile"], true, viewer),
        Some(0 | 1) => {
            if raw["artistInfo"].is_object() {
                artist(&raw["artistInfo"], &raw["userProfile"])
            } else {
                user(&raw["userProfile"], true, viewer)
            }
        }
        _ => None,
    }
}
