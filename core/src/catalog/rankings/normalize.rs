use super::*;
use crate::{Error, Result, protocol::api::identifier};
use serde_json::{Value, json};

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().into()
}
fn image(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}
pub(super) fn products(value: &Value) -> Result<Vec<AlbumSale>> {
    value["products"]
        .as_array()
        .ok_or(Error::Response("missing album sales products"))?
        .iter()
        .map(|r| {
            Ok(AlbumSale {
                id: identifier(&r["albumId"])?,
                name: text(&r["albumName"]),
                artist: text(&r["artistName"]),
                cover_url: image(&r["coverUrl"]),
                rank: r["rank"].as_i64(),
                rank_incr: r["rankIncr"].as_i64(),
                newest: r["newest"] == true,
                sale_num: r["saleNum"].as_u64(),
                certification: image(&r["salesCertificationSystemLevelCode"]),
            })
        })
        .collect()
}
pub(super) fn song_sales(value: &Value) -> Result<Vec<SongSale>> {
    value["data"]["itemList"]
        .as_array()
        .ok_or(Error::Response("missing song sales items"))?
        .iter()
        .map(|r| {
            Ok(SongSale {
                id: identifier(&r["songId"])?,
                name: text(&r["songName"]),
                artist: text(&r["artistName"]),
                cover_url: image(&r["coverUrl"]),
                quantity: r["quantity"].as_u64(),
                indicator: r["indicator"].as_f64(),
                rank_incr: r["rankIncr"].as_i64(),
                newest: r["newest"] == true,
                song: None,
            })
        })
        .collect()
}
pub(super) fn home(main: &Value, albums: &Value, songs: &Value) -> Result<RankingPage> {
    let albums = products(albums)?;
    let songs = song_sales(songs)?;
    let mut groups = Vec::new();
    for g in main["data"]
        .as_array()
        .ok_or(Error::Response("missing ranking groups"))?
    {
        let display = text(&g["displayType"]);
        if !["ONLY_COVER", "TOP_3"].contains(&display.as_str()) {
            continue;
        }
        let mut cards = Vec::new();
        for row in g["list"]
            .as_array()
            .ok_or(Error::Response("missing ranking cards"))?
        {
            if let Some(card) = card(row, &display)? {
                cards.push(card);
            }
        }
        if g["categoryCode"] == "OFFICIAL" {
            cards.push(special(
                "albumRank",
                "数字专辑畅销榜",
                "每30分钟更新",
                RankingTarget::AlbumSales,
                albums
                    .iter()
                    .map(|r| RankingPreview {
                        id: r.id.clone(),
                        rank: r.rank,
                        last_rank: r.rank.zip(r.rank_incr).and_then(|(a, b)| a.checked_add(b)),
                        name: r.name.clone(),
                        artist: r.artist.clone(),
                        cover_url: r.cover_url.clone(),
                        is_album: true,
                    })
                    .collect(),
            ));
            cards.push(special(
                "songIndexRank",
                "歌曲畅销指数榜",
                "每1小时更新",
                RankingTarget::SongSales,
                songs
                    .iter()
                    .map(|r| RankingPreview {
                        id: r.id.clone(),
                        rank: None,
                        last_rank: None,
                        name: r.name.clone(),
                        artist: r.artist.clone(),
                        cover_url: r.cover_url.clone(),
                        is_album: true,
                    })
                    .collect(),
            ));
        }
        if !cards.is_empty() {
            groups.push(RankingGroup {
                name: text(&g["name"]),
                display_type: display,
                cards,
            });
        }
    }
    Ok(RankingPage::Home { groups })
}
fn special(
    id: &str,
    name: &str,
    update: &str,
    target: RankingTarget,
    tracks: Vec<RankingPreview>,
) -> RankingCard {
    RankingCard {
        id: id.into(),
        name: name.into(),
        target,
        name_show_style: "singleRow".into(),
        update_frequency: update.into(),
        can_play: false,
        cover_url: tracks.first().and_then(|r| r.cover_url.clone()),
        text_cover: false,
        tracks,
    }
}
fn card(r: &Value, display: &str) -> Result<Option<RankingCard>> {
    if !["singleRow", "doubleRow"].contains(&r["nameShowStyle"].as_str().unwrap_or(""))
        || r["targetType"] != "PLAYLIST"
    {
        return Ok(None);
    }
    let mut tracks = Vec::new();
    if let (Some(ranks), Some(names)) = (r["trackRankList"].as_array(), r["tracks"].as_array()) {
        for (i, rank) in ranks.iter().enumerate() {
            let name = names
                .get(i)
                .ok_or(Error::Response("ranking preview length mismatch"))?;
            tracks.push(RankingPreview {
                id: identifier(&rank["trackId"])?,
                rank: rank["rank"].as_i64(),
                last_rank: rank["lastRank"].as_i64(),
                name: text(if name["first"].is_null() {
                    &rank["songName"]
                } else {
                    &name["first"]
                }),
                artist: text(if name["second"].is_null() {
                    &rank["artistName"]
                } else {
                    &name["second"]
                }),
                cover_url: image(&rank["coverImgUrl"]),
                is_album: rank["isAlbum"] == true,
            });
        }
    }
    let text_cover = display == "ONLY_COVER" && image(&r["newFirstTextCoverUrl"]).is_some();
    let cover_url = if display == "TOP_3" {
        image(&r["coverUrl"])
    } else {
        ["newFirstTextCoverUrl", "firstCoverUrl", "firstCoverHdUrl"]
            .iter()
            .find_map(|key| image(&r[*key]))
    };
    Ok(Some(RankingCard {
        id: identifier(&r["id"])?,
        target: RankingTarget::Playlist,
        name: text(&r["name"]),
        name_show_style: text(&r["nameShowStyle"]),
        update_frequency: text(&r["updateFrequency"]),
        can_play: r["canPlay"] == json!(true),
        cover_url,
        text_cover,
        tracks,
    }))
}
