use super::{
    PathRequest,
    clean::{clean, prefix, units},
};
use crate::{
    Error, Result,
    protocol::wire::serialization::{js_truthy, trim_js},
};
use serde_json::Value;

fn text(value: &Value) -> Result<&str> {
    if !js_truthy(value) {
        Ok("")
    } else {
        value.as_str().ok_or(Error::InvalidInput(
            "download metadata text must be a string",
        ))
    }
}
fn artists(value: &Value) -> Result<String> {
    if !js_truthy(value) {
        return Ok(String::new());
    }
    let array = value
        .as_array()
        .ok_or(Error::InvalidInput("download artists must be an array"))?;
    let mut names = Vec::new();
    for item in array.iter().take(3) {
        if !item.is_object() {
            return Err(Error::InvalidInput("download artist must be an object"));
        }
        let name = trim_js(text(&item["name"])?);
        if !name.is_empty() {
            names.push(name);
        }
    }
    Ok(names.join(","))
}
pub(super) fn components(r: &PathRequest) -> Result<Vec<String>> {
    let track = r.track.as_ref().unwrap_or(&Value::Null);
    let voice = r.voice.as_ref().unwrap_or(&Value::Null);
    let mv = r.mv.as_ref().unwrap_or(&Value::Null);
    let mut performer = String::new();
    let mut album_artist = String::new();
    let mut album = String::new();
    let named = r.yun_name.as_deref().filter(|s| !s.is_empty());
    let mut title: String = if let Some(name) = named {
        name.into()
    } else {
        let value = if js_truthy(&track["name"]) {
            &track["name"]
        } else {
            &voice["name"]
        };
        let name = trim_js(text(value)?);
        if name.is_empty() {
            text(&mv["title"])?.into()
        } else {
            name.into()
        }
    };
    let mut parts = Vec::new();
    if js_truthy(track) {
        performer = artists(&track["artists"])?;
        album_artist = artists(&track["album"]["artists"])?;
        if album_artist.is_empty() {
            album_artist = performer.clone();
        }
        album = trim_js(text(&track["album"]["name"])?).into();
    }
    if js_truthy(voice) {
        if js_truthy(&voice["audio"]) {
            album = trim_js(text(&voice["audio"]["name"])?).into();
        }
        if js_truthy(&voice["dj"]) {
            performer = trim_js(text(&voice["dj"]["nickname"])?).into();
            if album.is_empty() {
                album = trim_js(text(&voice["dj"]["brand"])?).into();
            }
        }
        if album.is_empty() {
            album = trim_js(text(&track["album"]["name"])?).into();
        }
        if performer.is_empty() {
            performer = artists(&track["artists"])?;
            if performer.is_empty() {
                performer = text(&voice["creator"]["nickname"])?.into();
            }
        }
        album_artist = performer.clone();
        parts.push("电台节目".into());
    }
    if js_truthy(mv) {
        performer = artists(&mv["artists"])?;
        album_artist = performer.clone();
        parts.push("MV".into());
    }
    if performer.is_empty() {
        performer = "未知歌手".into();
    }
    if album_artist.is_empty() {
        album_artist = "未知歌手".into();
    }
    if album.is_empty() {
        album = "未知专辑".into();
    }
    if title.is_empty() {
        title = "未知".into();
    }
    if js_truthy(mv) {
        album.clear();
    }
    match r.path_type {
        1 => parts.push(clean(&performer)),
        2 => {
            parts.push(clean(&album_artist));
            parts.push(clean(&album));
        }
        _ => (),
    }
    let filename = if named.is_some() {
        title
    } else {
        match r.file_type {
            0 => format!("{performer} - {title}"),
            1 => title,
            _ => format!("{title} - {performer}"),
        }
    };
    parts.push(clean(&filename));
    Ok(parts)
}
pub(super) fn fit(parts: &mut [String], budget: usize) -> Result<()> {
    if parts.iter().map(|s| units(s)).sum::<usize>() > budget {
        let last = parts.len() - 1;
        let head = budget / (parts.len() + 1);
        let mut used = 0;
        for (index, part) in parts.iter_mut().enumerate() {
            *part = clean(prefix(
                part,
                if index == last {
                    budget.saturating_sub(used)
                } else {
                    head
                },
            ));
            used += units(part);
        }
    }
    if parts.iter().any(|s| s.is_empty() || s == "." || s == "..") {
        return Err(Error::InvalidInput(
            "empty download path component after truncation",
        ));
    }
    Ok(())
}
