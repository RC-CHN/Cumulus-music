use super::{
    DiscoveryHomeCard as Card, DiscoveryHomeSection as Section, DiscoveryHomeTarget as Target,
    DiscoveryStyleSeeds,
    request::{
        ALL_LISTEN, BOOKS, CUSTOM, DAILY_VOICE, GENERAL, HEARTBEAT, HomeFeed, PLAYLISTS, STYLE,
        VIP, VOICES,
    },
};
use crate::{
    Error, Result,
    podcast::{Episode, Radio},
    protocol::{api::identifier, artwork::square},
};
use serde_json::Value;
use std::collections::HashSet;

const CARDS: usize = 18;
const PRIVATE_RADAR: &str = "3136952023";

fn rows(value: &Value) -> impl Iterator<Item = &Value> {
    value.as_array().into_iter().flatten().take(128)
}

pub(super) fn label(value: &Value) -> Option<String> {
    let value = value.as_str()?.trim();
    if value.len() > 768 || value.contains("://") || value.chars().any(char::is_control) {
        return None;
    }
    let value = value.chars().take(140).collect::<String>();
    (!value.is_empty()).then_some(value)
}

fn id(value: &Value) -> Option<String> {
    identifier(value).ok().filter(|id| id.len() <= 32)
}

fn art(values: &[&Value]) -> Option<String> {
    values.iter().find_map(|value| square(value))
}

fn section(
    kind: &'static str,
    title: String,
    cards: impl Iterator<Item = Card>,
) -> Option<Section> {
    let mut seen = HashSet::new();
    let cards = cards
        .filter(|card| seen.insert(card.target.clone()))
        .take(CARDS)
        .collect::<Vec<_>>();
    (!cards.is_empty()).then_some(Section { kind, title, cards })
}

fn heading(block: &Value, fallback: &str, subtitle: bool) -> String {
    let primary = &block["uiElement"][if subtitle { "subTitle" } else { "mainTitle" }]["title"];
    label(primary).unwrap_or_else(|| fallback.into())
}

fn playlist(creative: &Value) -> Option<Card> {
    let resource = &creative["resources"][0];
    if resource["valid"] == false
        || resource["resourceType"]
            .as_str()
            .is_some_and(|kind| !matches!(kind, "playlist" | "list"))
    {
        return None;
    }
    let id = id(&resource["resourceId"])?;
    Some(Card {
        title: label(&creative["uiElement"]["mainTitle"]["title"])?,
        subtitle: label(&creative["uiElement"]["subTitle"]["title"]),
        cover_url: art(&[
            &creative["uiElement"]["image"]["imageUrl"],
            &creative["uiElement"]["images"][0]["imageUrl"],
        ]),
        target: Target::Playlist { id },
    })
}

fn creatives(block: &Value) -> impl Iterator<Item = &Value> {
    rows(&block["creatives"])
}
fn resources(block: &Value) -> impl Iterator<Item = &Value> {
    creatives(block).flat_map(|row| rows(&row["resources"]))
}

fn song(resource: &Value) -> Option<Card> {
    let raw = &resource["resourceExtInfo"]["songData"];
    let song_id = id(&raw["id"])?;
    if resource["valid"] == false
        || (!resource["resourceId"].is_null() && id(&resource["resourceId"])? != song_id)
    {
        return None;
    }
    let album = raw
        .get("al")
        .or_else(|| raw.get("album"))
        .unwrap_or(&Value::Null);
    let artists = raw
        .get("ar")
        .or_else(|| raw.get("artists"))
        .unwrap_or(&Value::Null);
    let artists = rows(artists)
        .filter_map(|artist| label(&artist["name"]))
        .take(4)
        .collect::<Vec<_>>()
        .join(" / ");
    let ui = &resource["uiElement"]["subTitle"];
    Some(Card {
        title: label(&raw["name"])?,
        subtitle: if ui["titleType"] == "songRcmdTag" {
            label(&ui["title"])
        } else {
            (!artists.is_empty()).then_some(artists)
        },
        cover_url: square(&album["picUrl"]),
        target: Target::Song { id: song_id },
    })
}

fn voice(raw: &Value, expected: Option<&Value>, subtitle: Option<String>) -> Option<Card> {
    let episode = Episode::from_official(raw).ok()?;
    if episode.id.len() > 32
        || expected.is_some_and(|value| id(value).as_ref() != Some(&episode.id))
    {
        return None;
    }
    Some(Card {
        title: label(&Value::String(episode.name))?,
        subtitle: subtitle.or_else(|| label(&raw["radio"]["name"])),
        cover_url: square(&Value::String(episode.cover_url)),
        target: Target::Voice { id: episode.id },
    })
}

fn voice_resource(resource: &Value) -> Option<Card> {
    if resource["valid"] == false {
        return None;
    }
    let ui = &resource["uiElement"];
    let subtitle = (ui["labelType"] == "yellow")
        .then(|| label(&ui["labelTexts"][0]))
        .flatten();
    voice(
        &resource["resourceExtInfo"]["djProgram"],
        resource.get("resourceId"),
        subtitle,
    )
}

fn book(creative: &Value) -> Option<Card> {
    // M/21 K accepts a native audio detail identity from the complete radio
    // or from a voiceList creative. Never open a free-form block URL.
    let id = if let Ok(radio) = Radio::from_official(&creative["creativeExtInfoVO"]["radio"]) {
        (radio.id.len() <= 32).then_some(radio.id)?
    } else if creative["creativeType"] == "voiceList" {
        id(&creative["creativeId"])?
    } else {
        return None;
    };
    let radio = &creative["creativeExtInfoVO"]["radio"];
    let ui = &creative["uiElement"];
    Some(Card {
        title: label(&ui["mainTitle"]["title"])
            .or_else(|| label(&radio["name"]))
            .or_else(|| label(&radio["voiceListName"]))?,
        subtitle: label(&ui["subTitle"]["title"]),
        cover_url: art(&[
            &ui["image"]["imageUrl"],
            &radio["picUrl"],
            &radio["coverImgUrl"],
        ]),
        target: Target::Podcast { id },
    })
}

fn feature(row: &Value) -> Option<Card> {
    let module = row["moduleType"].as_str()?;
    let target = match (row["resourceType"].as_str()?, module) {
        ("playList", "radar" | "mood" | "tag_playlist") => Target::Playlist {
            id: id(&row["resourceId"])?,
        },
        ("toplist", "rank_board" | "hot_song_board" | "soar_board" | "new_song_board") => {
            Target::Playlist {
                id: id(&row["resourceId"])?,
            }
        }
        ("podcast", "daily_podcast") => Target::Podcast {
            id: id(&row["resourceId"])?,
        },
        ("fm", "fm") => Target::Fm,
        _ => return None,
    };
    Some(Card {
        title: label(&row["coverText"]).or_else(|| label(&row["title"]))?,
        subtitle: label(&row["title"]),
        cover_url: square(&row["coverUrl"]),
        target,
    })
}

fn style_feature(row: &Value, seeds: &mut DiscoveryStyleSeeds) -> Option<Card> {
    Some(Card {
        title: label(&row["coverText"]).or_else(|| label(&row["title"]))?,
        subtitle: label(&row["title"]),
        cover_url: square(&row["coverUrl"]),
        target: Target::Style {
            seed: seeds.add(row)?,
        },
    })
}

fn recent(row: &Value) -> Option<Card> {
    let target = match row["resourceType"].as_str()? {
        "userfm" => Target::Fm,
        "dailySongsTag" => return None, // The official style route carries additional seed context.
        "playlist" | "list" | "toplist" => Target::Playlist {
            id: id(&row["resourceId"])?,
        },
        "album" => Target::Album {
            id: id(&row["resourceId"])?,
        },
        "voice" | "voicelist" => Target::Podcast {
            id: id(&row["resourceId"])?,
        },
        _ => return None,
    };
    Some(Card {
        title: label(&row["title"])?,
        subtitle: label(&row["tag"]),
        cover_url: square(&row["coverUrlList"][0]),
        target,
    })
}

fn daily() -> Card {
    Card {
        title: "每日歌曲推荐".into(),
        subtitle: None,
        cover_url: None,
        target: Target::Daily,
    }
}

fn child<'a>(batch: &'a Value, path: &str) -> Option<&'a Value> {
    let response = batch.get(path)?;
    (response["code"].as_i64() == Some(200) || response["code"].as_str() == Some("200"))
        .then_some(&response["data"])
}

pub(super) fn home(
    feed: &HomeFeed,
    recent_data: Option<&Value>,
    seeds: &mut DiscoveryStyleSeeds,
) -> Result<Vec<Section>> {
    let general = child(&feed.batch, GENERAL)
        .and_then(|data| data["blocks"].as_array())
        .ok_or(Error::Response("missing homepage recommendation blocks"))?;
    let find = |code| general.iter().take(32).find(|row| row["blockCode"] == code);
    let mut result = Vec::new();
    let mix = feed.mix.as_ref().and_then(|reply| {
        let mut titles = HashSet::new();
        let data = &reply["data"];
        let mut section = section("featured", "精选推荐".into(), {
            // Normal items keep their position. The official wrapper supplies
            // its demote styles after normal items, never before.
            rows(&data["items"])
                .map(|row| (row, false))
                .chain(rows(&data["extInfo"]["pcRcmdDemoteItems"]).map(|row| (row, true)))
                .filter_map(|(row, demote)| {
                    if demote || row["moduleType"] == "tag_daily_rec" {
                        style_feature(row, seeds)
                    } else {
                        feature(row)
                    }
                })
                .filter(|card| titles.insert(card.title.clone()))
        })?;
        if section.cards.len() < 4 {
            return None;
        }
        section.cards.truncate(9);
        section.cards.insert(0, daily());
        Some(section)
    });
    // MainView's HomeFrameNew=t1 order puts playlist recommendations first.
    // The daily card joins this group only without the full featured carousel.
    if let Some(recommendations) = find(PLAYLISTS) {
        let radar = creatives(recommendations)
            .filter_map(playlist)
            .find(|card| {
                card.target
                    == Target::Playlist {
                        id: PRIVATE_RADAR.into(),
                    }
            });
        let ordinary = creatives(recommendations)
            .filter_map(playlist)
            .filter(|card| !matches!(&card.target, Target::Playlist { id } if id == PRIVATE_RADAR))
            .take(if mix.is_none() { 8 } else { 10 })
            .collect::<Vec<_>>();
        if !ordinary.is_empty() {
            let supplemental = mix
                .is_none()
                .then(daily)
                .into_iter()
                .chain(if mix.is_none() { radar } else { None });
            result.extend(section(
                "playlists",
                heading(recommendations, "推荐歌单", false),
                supplemental.chain(ordinary),
            ));
        }
    }
    result.extend(mix);
    let add_songs = |result: &mut Vec<_>, code, kind, title| {
        if let Some(block) = find(code) {
            let title = heading(block, title, true);
            let cleaned = if code == VIP {
                title
                    .replace("黑胶SVIP", "")
                    .replace("黑胶VIP", "")
                    .trim()
                    .to_owned()
            } else {
                title
            };
            let cleaned = if cleaned.is_empty() {
                "会员歌曲推荐".into()
            } else {
                cleaned
            };
            result.extend(section(kind, cleaned, resources(block).filter_map(song)));
        }
    };
    add_songs(&mut result, ALL_LISTEN, "all_listen", "新村民都爱听");
    if let Some(data) = recent_data {
        result.extend(section(
            "recent",
            "最近常听".into(),
            rows(&data["data"]["resources"]).take(12).filter_map(recent),
        ));
    }
    add_songs(
        &mut result,
        HEARTBEAT,
        "heartbeat",
        "你的红心歌曲和相似推荐",
    );
    if let Some(block) = child(&feed.batch, CUSTOM).and_then(|data| data["blocks"].get(0)) {
        result.extend(section(
            "radar",
            heading(block, "雷达歌单", false),
            creatives(block).filter_map(playlist),
        ));
    }
    add_songs(&mut result, VIP, "vip", "会员歌曲推荐");
    add_songs(&mut result, STYLE, "style", "相似风格歌曲推荐");
    if let Some(block) = find(DAILY_VOICE) {
        result.extend(section(
            "daily_voice",
            heading(block, "每天听点好音乐", true),
            rows(&block["creatives"][0]["resources"]).filter_map(voice_resource),
        ));
    }
    if let Some(data) = child(&feed.batch, VOICES) {
        let voices = rows(&data["recommendVoiceVOS"])
            .take(9)
            .filter_map(|row| voice(&row["djProgram"], None, label(&row["label"])))
            .collect::<Vec<_>>();
        // Official M/244 groups by three and omits an incomplete final group.
        let keep = voices.len() / 3 * 3;
        result.extend(section(
            "personal_voices",
            "你的专属播客".into(),
            voices.into_iter().take(keep),
        ));
    }
    if let Some(block) = child(&feed.batch, BOOKS).and_then(|data| data["blockVOS"].get(0)) {
        result.extend(section(
            "books",
            heading(block, "你的宝藏有声书", false),
            creatives(block).filter_map(book),
        ));
    }
    Ok(result)
}
