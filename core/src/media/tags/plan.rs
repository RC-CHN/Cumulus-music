use super::{MetadataKind, build_metadata, helpers::*};
use crate::{Error, Result, membership::Quality};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardTags {
    pub tit2: String,
    pub tpe1: String,
    pub talb: String,
    pub trck: String,
    pub tpos: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TagPlan {
    pub tags: StandardTags,
    pub comment_plain: String,
    pub cover_url: String,
    pub encrypted: bool,
}

/// Official M/754 + M/35 resource flags. This is a compatibility decision,
/// not authorization; plain authorized downloads remain the product default.
pub fn official_requires_encryption(flag: u32, payed: u32) -> bool {
    (flag & 4 != 0 && payed & 6 == 0) || flag & 32 != 0
}

/// Adapt raw song/resource fields to M/23's normalized task. `encrypted` records
/// the actual chosen output container, not a guessed membership entitlement.
pub fn track_plan(song: &Value, resource: &Value, encrypted: bool) -> Result<TagPlan> {
    let mut origin = song
        .as_object()
        .cloned()
        .ok_or(Error::InvalidInput("song must be an object"))?;
    if !resource.is_object() {
        return Err(Error::InvalidInput("resource must be an object"));
    }
    if !truth(song.get("id")) {
        return Err(Error::InvalidInput("song requires an ID"));
    }
    for (key, alias) in [
        ("album", "al"),
        ("artists", "ar"),
        ("duration", "dt"),
        ("mvid", "mv"),
        ("position", "no"),
    ] {
        if !origin.contains_key(key) {
            copy(&mut origin, key, song.get(alias));
        }
    }
    if !truth(origin.get("album")) {
        origin.insert("album".into(), json!({}));
    }
    if !truth(origin.get("artists")) {
        origin.insert("artists".into(), json!([]));
    }
    let mut input = json!({"origin":origin,"offlineId":format!("track-{}",string(song.get("id"))?),"encryptFile":encrypted});
    let object = input.as_object_mut().expect("object");
    for (key, from) in [
        ("realSuffix", "type"),
        ("md5", "md5"),
        ("volumeDelta", "gain"),
    ] {
        copy(object, key, resource.get(from));
    }
    if let Some(level) = resource["level"]
        .as_str()
        .and_then(|s| Quality::parse(s).ok())
    {
        object.insert("bitrate".into(), level.bitrate().into());
    } else {
        copy(object, "bitrate", resource.get("br"));
    }
    let result = build_metadata(&input, MetadataKind::Track, false)?
        .ok_or(Error::InvalidInput("missing metadata task"))?;
    if result["comm"].as_str().is_none_or(str::is_empty) {
        return Err(Error::InvalidInput("track does not support metadata tags"));
    }
    project(result, encrypted)
}

pub fn voice_plan(voice: &Value, resource: &Value, encrypted: bool) -> Result<TagPlan> {
    let input = json!({"origin":voice,"offlineId":format!("voice-{}",string(voice.get("id"))?),
        "realSuffix":resource["type"],"encryptFile":encrypted,"volumeDelta":resource["gain"]});
    let result = build_metadata(&input, MetadataKind::Voice, false)?
        .ok_or(Error::InvalidInput("missing voice metadata"))?;
    if result["comm"].as_str().is_none_or(str::is_empty) {
        return Err(Error::InvalidInput(
            "program does not support metadata tags",
        ));
    }
    let mut plan = project(result, encrypted)?;
    // M/23 ie([]) is a truthy JS array, so its standard artist tag can be
    // empty for voices. Preserve the exact dj: comment; supply a useful host
    // tag in the portable file adapter without changing the official builder.
    if plan.tags.tpe1.is_empty() {
        plan.tags.tpe1 = voice["dj"]["nickname"].as_str().unwrap_or("").into();
    }
    Ok(plan)
}

fn project(result: Value, encrypted: bool) -> Result<TagPlan> {
    let ext = &result["ext"];
    Ok(TagPlan {
        tags: StandardTags {
            tit2: text(ext.get("tit2"))?.into(),
            tpe1: if ext["tpe1"].is_array() {
                String::new()
            } else {
                text(ext.get("tpe1"))?.into()
            },
            talb: text(ext.get("talb"))?.into(),
            trck: text(ext.get("trck"))?.into(),
            tpos: text(ext.get("tpos"))?.into(),
        },
        comment_plain: result["comm"].as_str().expect("checked").into(),
        cover_url: text(result.get("picUrl"))?.into(),
        encrypted,
    })
}
