//! M/1211 allowlisted blocks; M/437 banner targets; M/400 ranking previews.
use super::{ZoneBlock, ZoneBlockKind as Block, ZoneCard, ZoneRankPreview, ZoneTab, ZoneTarget};
use crate::protocol::{
    api::identifier,
    artwork::{original, square},
};
use serde_json::Value;

fn rows(value: &Value) -> impl Iterator<Item = &Value> {
    value.as_array().into_iter().flatten().take(128)
}

fn text(value: &Value) -> Option<String> {
    safe_label(value.as_str()?.trim()).map(str::to_owned)
}

fn safe_label(text: &str) -> Option<&str> {
    (!text.is_empty()
        && text.len() <= 768
        && !text.contains("://")
        && !text.chars().any(char::is_control))
    .then_some(text)
}

fn marked(value: &Value) -> bool {
    // The recovered privacy gates use JavaScript truthiness, not strict === true.
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(value) => !value.is_empty(),
        _ => true,
    }
}

fn id(value: &Value) -> Option<String> {
    identifier(value).ok().filter(|id| id.len() <= 32)
}

pub(super) fn tab(value: &Value) -> Option<ZoneTab> {
    let key = value["tabKey"].as_str().filter(|s| super::valid_tab(s))?;
    Some(ZoneTab {
        key: key.into(),
        name: value["tabKeyName"]
            .as_str()
            .and_then(safe_label)
            .unwrap_or_default()
            .into(),
    })
}

pub(super) fn tab_metas(data: &Value) -> Vec<ZoneTab> {
    rows(&data["tabMetas"]).filter_map(tab).collect()
}

fn carousel(row: &Value) -> Option<ZoneCard> {
    let id = id(&row["targetId"])?;
    let target = match row["targetType"].as_u64()? {
        1 => ZoneTarget::Song { id },
        10 => ZoneTarget::Album { id },
        100 => ZoneTarget::Artist { id },
        1000 => ZoneTarget::Playlist {
            id,
            personalized: false,
        },
        1001 => ZoneTarget::Voice { id },
        1002 => ZoneTarget::User { id },
        1009 => ZoneTarget::Podcast { id },
        // M/437 also recognizes remote commerce, video with ambiguous
        // resource kind, advertising and live links. Do not guess identities
        // or pass those link destinations through the presentation DTO.
        _ => return None,
    };
    Some(ZoneCard {
        card_key: String::new(),
        title: text(&row["title"]),
        cover: original(&row["imgUrl"]),
        can_play: matches!(target, ZoneTarget::Song { .. } | ZoneTarget::Voice { .. }),
        mgc_special: false,
        target,
        previews: Vec::new(),
    })
}

fn recommendation(row: &Value, horizontal: bool, block_personalized: bool) -> Option<ZoneCard> {
    let personalized = block_personalized || marked(&row["hideAfterClosePersonalized"]);
    let target = match row["resourceType"].as_str()? {
        "playlist" => ZoneTarget::Playlist {
            id: id(&row["id"]).or_else(|| id(&row["resourceId"]))?,
            // M/3089 applies the specialized trial scene in Horizontal,
            // whereas the Grid uses the ordinary playlist presentation.
            personalized: horizontal && personalized,
        },
        "album" if horizontal => ZoneTarget::Album {
            id: id(&row["resourceId"])?,
        },
        "fm" if horizontal => ZoneTarget::Fm {
            id: id(&row["resourceId"])?,
            personalized,
        },
        _ => return None,
    };
    Some(ZoneCard {
        card_key: String::new(),
        title: text(&row["name"]).or_else(|| text(&row["title"])),
        cover: square(&row["coverImgUrl"]).or_else(|| square(&row["coverUrl"])),
        can_play: true,
        mgc_special: matches!(
            target,
            ZoneTarget::Playlist {
                personalized: true,
                ..
            }
        ) && row["specialType"].as_i64() == Some(100),
        target,
        previews: Vec::new(),
    })
}

fn ranking(row: &Value) -> Option<ZoneCard> {
    if row["targetType"] != "PLAYLIST"
        || !matches!(
            row["nameShowStyle"].as_str(),
            Some("singleRow" | "doubleRow")
        )
    {
        return None;
    }
    let tracks = row["trackRankList"].as_array()?;
    let labels = row["tracks"].as_array()?;
    let previews = tracks
        .iter()
        .zip(labels)
        .take(3)
        .filter_map(|(track, label)| {
            Some(ZoneRankPreview {
                title: text(&label["first"]).or_else(|| text(&track["songName"]))?,
                artist: text(&label["second"]).or_else(|| text(&track["artistName"])),
            })
        })
        .collect();
    Some(ZoneCard {
        card_key: String::new(),
        title: text(&row["name"]),
        cover: square(&row["coverUrl"]),
        target: ZoneTarget::Playlist {
            id: id(&row["id"])?,
            personalized: false,
        },
        can_play: marked(&row["canPlay"]),
        mgc_special: false,
        previews,
    })
}

pub(super) fn blocks(content: &Value, recommendation_enabled: bool) -> Vec<ZoneBlock> {
    rows(&content["blocks"])
        .filter_map(|module| {
            let kind = match module["componentName"].as_str()? {
                "RecommendTabCarousel" => Block::Carousel,
                "RecommendHorizontal" => Block::Horizontal,
                "MusiclibraryRank" => Block::Ranking,
                "RecommendGrid" => Block::Grid,
                _ => return None,
            };
            let children = module
                .get("children")
                .or_else(|| module.get("childrens"))
                .unwrap_or(&Value::Null);
            let personalized = marked(&module["hideAfterClosePersonalized"]);
            let requires_recommendation = personalized && !recommendation_enabled;
            let projected = rows(children)
                .filter_map(|row| match kind {
                    Block::Carousel => carousel(row),
                    Block::Ranking => ranking(row),
                    Block::Horizontal | Block::Grid => {
                        if !requires_recommendation
                            && !recommendation_enabled
                            && marked(&row["hideAfterClosePersonalized"])
                        {
                            return None;
                        }
                        recommendation(row, kind == Block::Horizontal, personalized)
                    }
                })
                .collect::<Vec<_>>();
            if projected.is_empty()
                || (!requires_recommendation && kind == Block::Horizontal && projected.len() < 6)
            {
                return None;
            }
            Some(ZoneBlock {
                kind,
                title: text(&module["title"]),
                requires_recommendation,
                cards: if requires_recommendation {
                    Vec::new()
                } else {
                    projected
                },
            })
        })
        .collect()
}
