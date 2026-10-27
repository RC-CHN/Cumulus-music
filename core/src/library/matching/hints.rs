use crate::{
    Error, Result,
    library::{LibraryItem, LocalRevision},
    media::{
        ncm::{HeaderPolicy, NcmReader},
        open_regular,
        tags::{COMMENT_PREFIX, decode_comment},
    },
    protocol::{
        api::identifier,
        wire::serialization::{js_string, js_truthy},
    },
};
use lofty::{config::ParseOptions, file::TaggedFileExt, probe::Probe, tag::ItemKey};
use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub enum MatchHint {
    Absent,
    /// An existing but unreadable/unsupported official comment is not silently
    /// replaced by a fuzzy match (M/2304's id3str branch also does not fall back).
    Invalid,
    Song(Value),
}
pub trait MetadataProbe: Send + Sync {
    fn inspect(&self, item: &LibraryItem) -> Result<MatchHint>;
}
pub struct FileMetadataProbe;
impl MetadataProbe for FileMetadataProbe {
    fn inspect(&self, item: &LibraryItem) -> Result<MatchHint> {
        // A container-level comment does not identify each CUE segment.
        if matches!(item.revision, LocalRevision::Cue { .. }) {
            return Ok(MatchHint::Absent);
        }
        if item.metadata.container == "ncm" {
            let reader = NcmReader::open(&item.path, HeaderPolicy::Compatible)?;
            return Ok(match &reader.info().metadata {
                Some(value) => metadata_song(
                    if value.get("programId").is_some() {
                        "dj"
                    } else {
                        "music"
                    },
                    value,
                )
                .map(MatchHint::Song)
                .unwrap_or(MatchHint::Invalid),
                None => MatchHint::Absent,
            });
        }
        let mut file = open_regular(&item.path)?;
        let probe = Probe::new(&mut file).guess_file_type()?;
        if probe.file_type().is_none() {
            return Ok(MatchHint::Absent);
        }
        let tagged = probe
            .options(
                ParseOptions::new()
                    .read_properties(false)
                    .read_cover_art(false),
            )
            .read()
            .map_err(|_| Error::Media("cannot read local matching metadata"))?;
        let mut found = None;
        for tag in tagged.tags() {
            for text in tag
                .get_strings(ItemKey::Comment)
                .chain(tag.get_strings(ItemKey::Description))
            {
                if !text.starts_with(COMMENT_PREFIX) {
                    continue;
                }
                let song = decode_comment(text)
                    .ok()
                    .and_then(|plain| decode_metadata(&plain));
                let Some(song) = song else {
                    return Ok(MatchHint::Invalid);
                };
                if found
                    .as_ref()
                    .is_some_and(|old: &Value| old["id"] != song["id"])
                {
                    return Ok(MatchHint::Invalid);
                }
                found = Some(song);
            }
        }
        Ok(found.map(MatchHint::Song).unwrap_or(MatchHint::Absent))
    }
}

/// M/23 E: decoded `music:`/`dj:` metadata becomes an online track. This parser
/// preserves decimal IDs as strings, including values beyond JS precision.
pub fn decode_metadata(plain: &str) -> Option<Value> {
    if plain.len() > 16 * 1024 * 1024 {
        return None;
    }
    let (kind, body) = plain.split_once(':')?;
    let value: Value =
        serde_json::from_str(body.trim_matches(|c: char| c.is_whitespace() || c.is_control()))
            .ok()?;
    metadata_song(kind, &value)
}
fn text(value: &Value) -> String {
    match value {
        Value::String(v) => v.clone(),
        Value::Number(v) => v.to_string(),
        _ => String::new(),
    }
}
fn truthy_text(value: &Value) -> String {
    if js_truthy(value) {
        text(value)
    } else {
        String::new()
    }
}
fn copy_fields(target: &mut Value, input: &Value, fields: &[(&str, &str)]) {
    for (to, from) in fields {
        if let Some(v) = input.get(from) {
            target[*to] = v.clone();
        }
    }
}
fn artists(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|pair| {
            let pair = pair.as_array().filter(|p| p.len() == 2)?;
            Some(json!({"name":pair[0], "id":js_string(&pair[1]).unwrap_or_default()}))
        })
        .collect()
}
fn metadata_song(kind: &str, value: &Value) -> Option<Value> {
    if !matches!(kind, "music" | "dj" | "mv") {
        return None;
    }
    let music = value
        .get("mainMusic")
        .filter(|v| js_truthy(v))
        .unwrap_or(value);
    let raw_id = text(&music["musicId"]);
    let id = if ["track-", "voice-", "mv-"]
        .iter()
        .any(|p| raw_id.to_ascii_lowercase().starts_with(p))
    {
        raw_id.split_once('-')?.1.to_owned()
    } else {
        raw_id
    };
    let id = identifier(&json!(id)).ok()?;
    let bitrate = music["bitrate"].as_f64().map(|v| {
        if v <= 0.0 {
            0.0
        } else if v <= 100_000.0 {
            64_000.0
        } else if v <= 150_000.0 {
            128_000.0
        } else if v <= 300_000.0 {
            192_000.0
        } else if v <= 400_000.0 {
            320_000.0
        } else if v < 1_999_000.0 {
            999_000.0
        } else if v < 2_999_000.0 {
            1_999_000.0
        } else if v < 3_999_000.0 {
            2_999_000.0
        } else {
            v
        }
    });
    let mut song = json!({"id":id,
        "artists":artists(&music["artist"]), "album":{
            "id":truthy_text(&music["albumId"]), "name":text(&music["album"]),
            "picId":truthy_text(&music["albumPicDocId"]), "picUrl":truthy_text(&music["albumPic"])},
        "transNames":music.get("transNames").filter(|v| js_truthy(v)).cloned().unwrap_or(json!([])),
        "mvId":truthy_text(&music["mvId"]),
        "realSuffix":music["format"].as_str().filter(|v| !v.is_empty()).unwrap_or("mp3"),
        "commentThreadId":text(&music["commentThreadId"])});
    copy_fields(
        &mut song,
        music,
        &[("name", "musicName"), ("duration", "duration")],
    );
    for field in ["alias", "volumeDelta"] {
        if let Some(v) = music.get(field) {
            song[field] = v.clone();
        }
    }
    if let Some(fee) = music.get("fee").filter(|v| !v.is_null()) {
        song["fee"] = fee.clone();
    }
    if music.get("flag").is_none() {
        song["privilege"] = json!({});
    }
    if let Some(v) = bitrate {
        song["bitrate"] = json!(v);
    }
    if let Some(flag) = music
        .pointer("/privilege/flag")
        .filter(|v| !v.is_null())
        .or_else(|| music.get("flag").filter(|v| !v.is_null()))
    {
        song["privilege"] = json!({"flag":flag});
    }
    if kind == "dj" {
        let program = identifier(&value["programId"]).ok()?;
        let radio = if value.get("radioId").is_some() {
            value
        } else {
            &value["radio"]
        };
        song["voiceId"] = json!(program);
        song["voice"] = json!({"id":program,
            "creator":{}, "mainTrackId":id,
            "description":value["programDesc"].as_str().unwrap_or(""),
            "duration":music["duration"].as_f64().unwrap_or(0.0),
            "audio":{"id":text(&radio["radioId"]),
                "desc":radio.get("radioDesc").or_else(|| radio.get("introduction")).cloned().unwrap_or(json!("")),
                "category":radio["radioCategory"].as_str().unwrap_or("")}});
        copy_fields(
            &mut song["voice"],
            value,
            &[
                ("name", "programName"),
                ("serialNum", "serial"),
                ("createTime", "createTime"),
            ],
        );
        copy_fields(
            &mut song["voice"]["creator"],
            value,
            &[
                ("userId", "djId"),
                ("nickname", "djName"),
                ("brand", "brand"),
                ("avatarUrl", "djAvatarUrl"),
            ],
        );
        copy_fields(
            &mut song["voice"]["audio"],
            radio,
            &[
                ("name", "radioName"),
                ("categoryId", "radioCategoryId"),
                ("radioFeeType", "radioFeeType"),
                ("feeScope", "radioFeeScope"),
            ],
        );
        if let Some(cover) = music
            .get("albumPic")
            .filter(|v| js_truthy(v))
            .or_else(|| music.pointer("/audio/coverImgUrl").filter(|v| js_truthy(v)))
            .or_else(|| music.pointer("/dj/blurCoverUrl"))
        {
            song["voice"]["coverUrl"] = cover.clone();
        }
        song["artists"] = json!([{"name":text(&value["djName"]),
            "id":text(&value["djId"]), "userId":text(&value["djId"])}]);
        song["volumeDelta"] = value.get("volumeDelta").cloned().unwrap_or(json!(0));
        if song["album"]["name"] == "" {
            song["album"]["name"] = json!(text(&radio["radioName"]));
        }
    }
    if kind == "mv" {
        // Preserve the recovered MV metadata for native consumers. Cover URL
        // resolution is a separate authenticated catalog operation, never file I/O.
        song["mv"] = json!({"id":text(&music["mvId"]), "title":music["title"],
            "desc":music["briefIntro"], "briefDesc":music["detailIntro"],
            "pubTime":music["pubTime"], "coverId":text(&music["picId"]),
            "duration":music["duration"], "artistName":music["artistName"],
            "artists":artists(&music["artist"]), "privilege":music["privilege"]});
        song["bitrate"] = music["bitrate"].clone();
    }
    Some(song)
}
