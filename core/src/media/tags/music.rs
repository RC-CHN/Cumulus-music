use super::helpers::*;
use crate::{Result, protocol::wire::serialization::js_json};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

pub(super) fn music(input: &Value, origin: &Value, id: &str) -> Result<Map<String, Value>> {
    static ID: LazyLock<Regex> =
        LazyLock::new(|| Regex::new("(?i)offline-|track-").expect("static regex"));
    let album = &origin["album"];
    let mut m = Map::new();
    m.insert("musicId".into(), ID.replace_all(id, "").to_string().into());
    m.insert("musicName".into(), trimmed(origin.get("name"))?.into());
    m.insert("artist".into(), artists(origin.get("artists"))?.into());
    copy(&mut m, "albumId", album.get("id"));
    m.insert("album".into(), trimmed(album.get("name"))?.into());
    copy(&mut m, "albumPicDocId", album.get("picId"));
    copy(&mut m, "albumPic", album.get("picUrl"));
    for (to, from) in [("bitrate", "bitrate"), ("mp3DocId", "md5")] {
        copy(&mut m, to, input.get(from));
    }
    for (to, from) in [
        ("duration", "duration"),
        ("mvId", "mvid"),
        ("alias", "alias"),
    ] {
        copy(&mut m, to, origin.get(from));
    }
    m.insert(
        "transNames".into(),
        default(origin.get("transNames"), json!([])),
    );
    copy(&mut m, "format", input.get("realSuffix"));
    copy(&mut m, "fee", origin.get("fee"));
    m.insert(
        "volumeDelta".into(),
        default(input.get("volumeDelta"), json!(0)),
    );
    Ok(m)
}

pub(super) fn mv(input: &Value, origin: &Value) -> Result<Value> {
    let mut m = Map::new();
    copy(&mut m, "title", origin.get("title"));
    copy(&mut m, "mvId", origin.get("id"));
    m.insert("artist".into(), artists(origin.get("artists"))?.into());
    for (to, from, empty) in [
        ("artistName", "artistName", json!("")),
        ("pubTime", "pubTime", json!("")),
        ("bitrate", "bitrate", json!(0)),
        ("duration", "duration", json!(0)),
        ("briefIntro", "briefDesc", json!("")),
        ("detailIntro", "desc", json!("")),
    ] {
        m.insert(
            to.into(),
            default(
                if to == "bitrate" {
                    input.get(from)
                } else {
                    origin.get(from)
                },
                empty,
            ),
        );
    }
    let mut privilege = Map::new();
    for key in ["fee", "payed"] {
        copy(&mut privilege, key, origin["privilege"].get(key));
    }
    m.insert("privilege".into(), privilege.into());
    copy(&mut m, "format", input.get("realSuffix"));
    m.insert("picId".into(), default(origin.get("coverId"), json!(0)));
    m.insert("picUrl".into(), default(origin.get("coverUrl"), json!("")));
    copy(&mut m, "type", origin.get("type"));
    Ok(m.into())
}

pub(super) fn comment(prefix: &str, value: Value, numeric_document: bool) -> Result<String> {
    static DOCUMENT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#""mp3DocId":"([0-9]+)""#).expect("static regex"));
    let plain = format!("{prefix}:{}", js_json(&value)?);
    Ok(if numeric_document {
        DOCUMENT.replace(&plain, r#""mp3DocId":$1"#).into_owned()
    } else {
        plain
    })
}
