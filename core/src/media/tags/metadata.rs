use super::{helpers::*, music, voice};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MetadataKind {
    Track,
    Voice,
    Mv,
}

/// Execute the recovered M/23.le contract on an official normalized task.
/// `special_track` is the caller's M/track predicate result; it is not an
/// entitlement override. This function only produces metadata, with no IO.
pub fn build_metadata(
    input: &Value,
    kind: MetadataKind,
    special_track: bool,
) -> Result<Option<Value>> {
    if !truth(input.get("offlineId")) {
        return Ok(None);
    }
    let id = input["offlineId"]
        .as_str()
        .ok_or(Error::InvalidInput("offline ID must be text"))?;
    let origin = &input["origin"];
    let suffix = text(input.get("realSuffix"))?;
    let media = input
        .get("finalPath")
        .filter(|v| truth(Some(v)))
        .or_else(|| input.get("path"));
    let mut ext = Map::new();
    copy(&mut ext, "media_rel_path", media);
    ext.insert("image_rel_path".into(), json!(""));
    let empty = |image_id: String, comm: String| json!({"id":image_id,"offlineId":id,"lyricId":0,"picUrl":"","tmpPicUrl":"","comm":comm,"ext":ext});
    if !["mp3", "flac", "aac", "m4a", "mp4"].contains(&suffix.to_ascii_lowercase().as_str())
        || origin["songType"].as_u64() == Some(1)
    {
        return Ok(Some(empty(format!("{id}-image"), String::new())));
    }
    if !origin.is_object() {
        return Err(Error::InvalidInput("metadata origin must be an object"));
    }
    if special_track {
        return Ok(Some(empty(
            format!("{}-image", string(origin.get("id"))?),
            music::comment("music", music::music(input, origin, id)?.into(), false)?,
        )));
    }
    if kind == MetadataKind::Mv {
        let mut ext = Map::new();
        ext.insert("tpe1".into(), artist_text(origin.get("artists"))?);
        ext.insert("tit2".into(), default(origin.get("title"), json!("")));
        copy(&mut ext, "media_rel_path", media);
        return Ok(Some(
            json!({"id":format!("{}-image",string(origin.get("id"))?),"offlineId":id,"comm":music::comment("mv",music::mv(input,origin)?,true)?,"ext":ext}),
        ));
    }
    let mut track = if kind == MetadataKind::Track {
        Some(music::music(input, origin, id)?)
    } else {
        None
    };
    if truth(input.get("encryptFile")) && truth(origin.get("privilege")) {
        let mut p = Map::new();
        copy(&mut p, "flag", origin["privilege"].get("flag"));
        track
            .get_or_insert_with(Map::new)
            .insert("privilege".into(), p.into());
    }
    let voice = (kind == MetadataKind::Voice).then(|| voice::voice(input, origin));
    let mut ext = Map::new();
    ext.insert("tit2".into(), trimmed(origin.get("name"))?.into());
    let artist = artist_text(origin.get("artists"))?;
    if !truth(Some(&artist)) {
        copy(
            &mut ext,
            "tpe1",
            voice.as_ref().and_then(|v| v.get("djName")),
        );
    } else {
        ext.insert("tpe1".into(), artist);
    }
    if let Some(radio) = voice
        .as_ref()
        .and_then(|v| v.get("radioName"))
        .filter(|v| truth(Some(v)))
    {
        ext.insert("talb".into(), radio.clone());
    } else {
        if !origin["album"].is_object() {
            return Err(Error::InvalidInput(
                "normalized metadata requires an album object",
            ));
        }
        copy(&mut ext, "talb", origin["album"].get("name"));
    }
    ext.insert("encrypt".into(), truth(input.get("encryptFile")).into());
    copy(&mut ext, "media_rel_path", media);
    ext.insert("image_rel_path".into(), json!(""));
    for (to, from) in [("trck", "position"), ("tpos", "cd")] {
        ext.insert(
            to.into(),
            if truth(origin.get(from)) {
                string(origin.get(from))?
            } else {
                String::new()
            }
            .into(),
        );
    }
    let mut out = Map::new();
    out.insert("id".into(), format!("{id}-image").into());
    out.insert("offlineId".into(), id.into());
    if kind == MetadataKind::Voice {
        out.insert("lyricId".into(), json!(0));
    } else {
        copy(
            &mut out,
            "lyricId",
            track.as_ref().and_then(|t| t.get("musicId")),
        );
    }
    let pic = if kind == MetadataKind::Voice {
        origin.get("coverUrl")
    } else {
        track.as_ref().and_then(|t| t.get("albumPic"))
    };
    copy(&mut out, "picUrl", pic);
    out.insert(
        "tmpPicUrl".into(),
        format!(
            "meta\\{id}.{}",
            text(pic)?.split('.').next_back().unwrap_or("")
        )
        .into(),
    );
    let comm = if let Some(track) = track {
        music::comment("music", track.into(), true)?
    } else {
        music::comment("dj", voice.expect("voice kind"), true)?
    };
    out.insert("comm".into(), comm.into());
    out.insert("ext".into(), ext.into());
    Ok(Some(out.into()))
}
