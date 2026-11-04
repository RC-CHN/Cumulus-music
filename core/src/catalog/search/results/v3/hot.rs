//! M/21 hot-explanation normalization. Only fixed internal detail destinations are executable.
use super::{image, text};
use crate::protocol::api::identifier;
use serde_json::{Value, json};

fn target(kind: &str, id: &Value) -> Option<Value> {
    let kind = match kind {
        "album" | "artist" | "playlist" | "mv" => kind,
        "djradio" | "voicelist" => "podcast",
        _ => return None,
    };
    let id = identifier(id).ok().filter(|id| id.len() <= 32)?;
    Some(json!({"kind":kind,"id":id}))
}

fn explicit(action: &str) -> Option<Value> {
    if action.len() > 2048 || action.contains('#') || action.chars().any(char::is_control) {
        return None;
    }
    let (kind, id) = action.strip_prefix("orpheus://")?.split_once('/')?;
    let id = id.split(['?', '&']).next()?;
    target(kind, &json!(id))
}

fn related(resource: &Value) -> Option<(&str, &Value)> {
    let kind = resource["resourceType"].as_str()?;
    let base = &resource["baseInfo"];
    let (kind, data) = match kind {
        "album" => (kind, &base["albumData"]),
        "artist" => (kind, &base["artistDTO"]),
        "playlist" => (kind, &base["pubPlaylistData"]),
        "voicelist" => (kind, &base["pubDJProgramData"]),
        "new_mlog" if resource["subType"] == "mv" => ("mv", &base["squareFeedDto"]),
        _ => return None,
    };
    Some((kind, data))
}

fn related_target(resource: &Value) -> Option<Value> {
    let (kind, data) = related(resource)?;
    let ids = match kind {
        "album" => ["id", "albumId", "resourceId", ""],
        "voicelist" => ["id", "resourceId", "voiceListId", "productId"],
        _ => ["id", "resourceId", "", ""],
    };
    ids.into_iter()
        .filter(|key| !key.is_empty())
        .find_map(|key| {
            data.get(key)
                .or_else(|| resource.get(key))
                .and_then(|id| target(kind, id))
        })
}

fn related_picture(resource: &Value) -> Option<Value> {
    let (kind, data) = related(resource)?;
    ["coverImgUrl", "picUrl", "coverUrl", "cover"]
        .into_iter()
        .find_map(|key| image(&data[key]))
        .or_else(|| {
            (kind == "mv")
                .then(|| image(&data["resource"]["mlogBaseData"]["coverUrl"]))
                .flatten()
        })
}

fn heat(value: &Value) -> Option<String> {
    let n = if let Some(text) = value.as_str().filter(|text| text.len() <= 32) {
        let split = text.find(|c: char| !c.is_ascii_digit() && c != '.');
        if let Some(split) = split {
            let (number, unit) = text.split_at(split);
            if number
                .parse::<f64>()
                .is_ok_and(|v| v.is_finite() && v >= 0.0)
                && !unit.is_empty()
                && unit.chars().all(|c| "十百千万亿".contains(c))
            {
                return Some(text.into());
            }
            return None;
        }
        text.parse::<u64>().ok()?
    } else {
        value.as_u64()?
    };
    if n >= 1 << 53 {
        return None;
    }
    Some(if n > 100_000_000 {
        format!("{}亿", (n / 10_000_000) as f64 / 10.0)
    } else if n > 100_000 {
        format!("{}万", (n / 1_000) as f64 / 10.0)
    } else {
        n.to_string()
    })
}

pub(super) fn explanation(block: &Value) -> Option<Value> {
    let first = block["resources"].as_array()?.first()?;
    let data = &first["baseInfo"]["hotExplainModuleData"];
    let title = text(&data["title"], 160)?;
    let content = text(&data["text"], 600)?;
    let related = first["relatedResources"]
        .as_array()
        .and_then(|rows| rows.first());
    let mut result = json!({"title":title,"text":content});
    if let Some(picture) = image(&data["pictureUrl"]).or_else(|| related.and_then(related_picture))
    {
        result["pictureUrl"] = picture;
    }
    if let Some(corner) = text(&data["pictureCornerMarker"], 16) {
        result["pictureCornerMarker"] = json!(corner);
    }
    if let Some(heat) = heat(&data["hotValue"]) {
        result["hotValue"] = json!(heat);
    }
    if data["canClick"] == true {
        let action = match &data["textAction"] {
            Value::Null => related.and_then(related_target),
            Value::String(value) if value.is_empty() => related.and_then(related_target),
            Value::String(value) => explicit(value),
            _ => None,
        };
        if let Some(target) = action {
            result["action"] = json!({"target":target,"label":text(&data["urlText"], 40).unwrap_or_else(|| "查看详情".into())});
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_target_wins_and_related_resource_never_overrides_a_blocked_action() {
        let mut block = json!({"resources":[{"baseInfo":{"hotExplainModuleData":{
            "title":"热点","text":"说明","textAction":"orpheus://album/28?tracking=PRIVATE","canClick":true,
            "pictureCornerMarker":"热门","hotValue":253_567,"urlText":"查看专辑"}},
            "relatedResources":[{"resourceType":"playlist","baseInfo":{"pubPlaylistData":{
                "id":"29","coverImgUrl":"https://p1.music.126.net/pic?auth=PRIVATE"}}}]
        }]});
        let card = explanation(&block).unwrap();
        assert_eq!(
            card["action"],
            json!({"label":"查看专辑","target":{"kind":"album","id":"28"}})
        );
        assert_eq!(card["hotValue"], "25.3万");
        assert_eq!(card["pictureCornerMarker"], "热门");
        assert_eq!(
            card["pictureUrl"],
            "https://p1.music.126.net/pic?param=320y320"
        );
        assert!(!card.to_string().contains("PRIVATE"));

        for unsafe_link in [
            "https://music.163.com/playlist?id=29",
            "orpheus://rnpage?component=private",
            "orpheus://playlist/29/30",
            "orpheus://playlist/29#x",
            "orpheus://playlist/0",
            "orpheus://song/29",
            "orpheus://playlist/000000000000000000000000000000001",
        ] {
            block["resources"][0]["baseInfo"]["hotExplainModuleData"]["textAction"] =
                json!(unsafe_link);
            assert!(
                explanation(&block).unwrap()["action"].is_null(),
                "{unsafe_link}"
            );
        }
        block["resources"][0]["baseInfo"]["hotExplainModuleData"]["textAction"] = Value::Null;
        let card = explanation(&block).unwrap();
        assert_eq!(
            card["action"]["target"],
            json!({"kind":"playlist","id":"29"})
        );
        block["resources"][0]["baseInfo"]["hotExplainModuleData"]["canClick"] = json!(false);
        assert!(explanation(&block).unwrap()["action"].is_null());
    }
}
