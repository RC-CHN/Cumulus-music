//! M/21 hot-explanation normalization for internal details and verified zones.
mod vertical_zone;
use super::{image, text};
use crate::protocol::api::identifier;
use serde_json::{Value, json};
pub(crate) use vertical_zone::HotZoneContext;
pub use vertical_zone::{SearchHotZoneSource, SearchHotZoneTarget};

fn target(kind: &str, id: &Value) -> Option<Value> {
    let kind = match kind {
        "album" | "artist" | "playlist" | "mv" | "song" => kind,
        "voice" | "program" => "voice",
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
    if kind == "voice" {
        // M/21 names the URL form program; voice is the related-resource type.
        return None;
    }
    let id = id.split(['?', '&']).next()?;
    target(kind, &json!(id))
}

fn related(resource: &Value) -> Option<(&str, &Value)> {
    let kind = resource["resourceType"].as_str()?;
    let base = &resource["baseInfo"];
    let (kind, data) = match kind {
        "song" => (kind, &base["simpleSongData"]),
        "voice" => (kind, &base["pubDJProgramData"]),
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
        "voice" => ["id", "resourceId", "voiceId", "productId"],
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
            if kind != "song" {
                return None;
            }
            ["album", "al"].into_iter().find_map(|key| {
                ["picUrl", "cover", "coverUrl"]
                    .into_iter()
                    .find_map(|art| image(&data[key][art]))
            })
        })
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

pub(super) fn explanation(block: &Value, zones: HotZoneContext<'_>) -> Option<Value> {
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
    let target = match &data["textAction"] {
        Value::Null => related.and_then(related_target),
        Value::String(value) if value.is_empty() => related.and_then(related_target),
        Value::String(value) => match vertical_zone::action(value, zones) {
            vertical_zone::ZoneAction::Other => explicit(value),
            // M/21 drops the RN hotspot entirely when its zone is unsupported.
            vertical_zone::ZoneAction::Blocked => return None,
            vertical_zone::ZoneAction::Allowed(zone) => {
                let mut target = serde_json::to_value(zone).ok()?;
                target["kind"] = json!("vertical_zone");
                Some(target)
            }
        },
        _ => None,
    };
    if let Some(target) = target {
        if matches!(target["kind"].as_str(), Some("song" | "voice")) {
            // The cover is an independent Play/Pause/Resume control in
            // HotExplainCard. The text action has a separate canClick gate.
            result["playTarget"] = target.clone();
        }
        if data["canClick"] == true {
            result["action"] = json!({"target":target,"label":text(&data["urlText"], 40).unwrap_or_else(|| "查看详情".into())});
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{
        pc_experiments::PcExperimentGroups, wire::serialization::encode_component,
    };

    fn explanation(block: &Value) -> Option<Value> {
        super::explanation(block, HotZoneContext::default())
    }

    fn zone_groups(group: &str) -> PcExperimentGroups {
        PcExperimentGroups::refreshed(&json!({"data":[
            {"expName":"PH-PC-zhuanqu","expGroupName":group}
        ]}))
        .unwrap()
    }

    fn zone_block(action: &str, can_click: bool) -> Value {
        json!({"resources":[{
            "baseInfo":{"hotExplainModuleData":{
                "title":"热点","text":"说明","textAction":action,"canClick":can_click,
                "urlText":"查看专区"
            }},
            "relatedResources":[{"resourceType":"song","baseInfo":{"simpleSongData":{"id":17}}}]
        }]})
    }

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
            "orpheus://playlist/29/30",
            "orpheus://playlist/29#x",
            "orpheus://playlist/0",
            "orpheus://song/29/30",
            "orpheus://voice/29",
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
    #[test]
    fn playable_hotspots_separate_cover_authority_from_the_detail_gate() {
        let mut block = json!({"resources":[{
            "baseInfo":{"hotExplainModuleData":{
                "title":"热点","text":"说明","textAction":"orpheus://song/17?trace=PRIVATE","canClick":true
            }},
            "relatedResources":[{"resourceType":"voice","baseInfo":{"pubDJProgramData":{
                "id":"401","mainSong":{"id":17},"coverUrl":"https://p1.music.126.net/cover?auth=PRIVATE"
            }}}]
        }]});
        let card = explanation(&block).unwrap();
        assert_eq!(card["playTarget"], json!({"kind":"song","id":"17"}));
        assert_eq!(card["action"]["target"], card["playTarget"]);
        assert!(!card.to_string().contains("PRIVATE"));

        block["resources"][0]["baseInfo"]["hotExplainModuleData"]["textAction"] = json!("");
        block["resources"][0]["baseInfo"]["hotExplainModuleData"]["canClick"] = json!(false);
        let card = explanation(&block).unwrap();
        assert_eq!(card["playTarget"], json!({"kind":"voice","id":"401"}));
        assert!(card["action"].is_null());

        block["resources"][0]["baseInfo"]["hotExplainModuleData"]["textAction"] =
            json!("https://example.invalid/PRIVATE");
        let card = explanation(&block).unwrap();
        assert!(card["playTarget"].is_null());
        assert!(card["action"].is_null());

        block["resources"][0]["baseInfo"]["hotExplainModuleData"]["textAction"] =
            json!("orpheus://song/");
        let card = explanation(&block).unwrap();
        assert!(card["playTarget"].is_null());
        assert!(card["action"].is_null());
    }

    #[test]
    fn zone_action_keeps_only_the_registered_page_and_resolved_tab_name() {
        let group = zone_groups("t1");
        let zones = HotZoneContext {
            groups: Some(&group),
            ready_pages: Some(&|code| code == "Rock_12"),
        };
        let name = "独立音乐 + 100% & A";
        let encoded = encode_component(&encode_component(&encode_component(name)));
        let link = format!(
            "orpheus://rnpage/discard_7?component=ignored&%63omponent=cube-renderer-rn&page=first&page=Rock_12&tabKey=first&tabKey={encoded}&from=PRIVATE&url=https%3A%2F%2Fprivate.invalid"
        );
        let projected = super::explanation(&zone_block(&link, true), zones).unwrap();
        let target = json!({
            "kind":"vertical_zone", "pageCode":"Rock_12", "tabKeyName":name, "from":"searchHotExplain"
        });
        assert_eq!(
            projected["action"],
            json!({"label":"查看专区","target":target})
        );
        assert!(projected["playTarget"].is_null());
        let serialized = projected.to_string();
        assert!(!serialized.contains("PRIVATE") && !serialized.contains("private.invalid"));
        assert!(!serialized.contains("discard_7") && !serialized.contains("orpheus://"));

        // The public result DTO accepts the flat typed action and only the
        // fixed source. A tab name that resembles a key is still a name.
        let action: crate::client::dto::SearchHotAction =
            serde_json::from_value(projected["action"].clone()).unwrap();
        assert_eq!(serde_json::to_value(action).unwrap()["target"], target);
        let mut forged = projected["action"].clone();
        forged["target"]["from"] = json!("anotherSource");
        assert!(serde_json::from_value::<crate::client::dto::SearchHotAction>(forged).is_err());

        let projected = super::explanation(
            &zone_block(
                "orpheus://rnpage&?component=cube-renderer-rn&page=Rock_12&tabKey=不使用&tabKey=",
                true,
            ),
            zones,
        )
        .unwrap();
        assert_eq!(
            projected["action"]["target"],
            json!({
                "kind":"vertical_zone", "pageCode":"Rock_12", "from":"searchHotExplain"
            })
        );
        let projected = super::explanation(
            &zone_block(
                "orpheus://rnpage?component=cube-renderer-rn&page=Rock_12&tabKey=A%2BB+%25bad",
                true,
            ),
            zones,
        )
        .unwrap();
        assert_eq!(projected["action"]["target"]["tabKeyName"], "A+B %bad");
        let read_only = super::explanation(&zone_block(&link, false), zones).unwrap();
        assert!(read_only["action"].is_null() && read_only["playTarget"].is_null());
    }

    #[test]
    fn zone_gate_drops_the_card_without_falling_back_to_its_song() {
        let allowed = zone_groups("t1");
        let control = zone_groups("c");
        let valid = zone_block(
            "orpheus://rnpage?component=cube-renderer-rn&page=Rock_12",
            true,
        );
        for zones in [
            HotZoneContext::default(),
            HotZoneContext {
                groups: Some(&allowed),
                ready_pages: None,
            },
            HotZoneContext {
                groups: Some(&control),
                ready_pages: Some(&|_| true),
            },
            HotZoneContext {
                groups: None,
                ready_pages: Some(&|_| true),
            },
            HotZoneContext {
                groups: Some(&allowed),
                ready_pages: Some(&|code| code == "rock_12"),
            },
        ] {
            assert!(super::explanation(&valid, zones).is_none());
        }
        let zones = HotZoneContext {
            groups: Some(&allowed),
            ready_pages: Some(&|_| true),
        };
        for link in [
            "orpheus://rnpage?component=private&page=Rock_12",
            "orpheus://rnpage?component=cube-renderer-rn&component=private&page=Rock_12",
            "orpheus://rnpage/Rock_12?component=cube-renderer-rn",
            "orpheus://rnpage/1/2?component=cube-renderer-rn&page=Rock_12",
            "orpheus://rnpage?component=cube-renderer-rn&page=Rock%252D12",
            "orpheus://rnpage?component=cube-renderer-rn&page=..%2FRock_12",
            "orpheus://rnpage?component=cube-renderer-rn&page=Rock_12#private",
            "orpheus://rnpage?component=cube-renderer-rn&page=Rock_12&tabKey=%250A",
            "orpheus://rnpage?component=cube-renderer-rn&page=Rock_12&tabKey=https%3A%2F%2Fprivate.invalid",
        ] {
            assert!(
                super::explanation(&zone_block(link, true), zones).is_none(),
                "{link}"
            );
        }
    }
}
