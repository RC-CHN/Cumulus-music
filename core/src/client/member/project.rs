use super::{super::mapping, dto::*};
use crate::{Error, Result, protocol::api::identifier};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) struct Projection {
    pub page: MemberPage,
    pub links: BTreeMap<String, String>,
}
impl Projection {
    pub fn revision(&self) -> Result<String> {
        use sha2::{Digest, Sha256};
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(
            &self.page,
            &self.links,
        ))?)))
    }
}
pub(super) fn validate_songs(values: &[Value]) -> Result<()> {
    if values.len() > 5000 {
        return Err(Error::Response("recommendation history too large"));
    }
    for value in values {
        mapping::track(value)?;
    }
    Ok(())
}
fn text(v: &Value) -> String {
    mapping::string(v)
}
fn rows(v: &Value) -> Result<&[Value]> {
    match v {
        Value::Null => Ok(&[]),
        Value::Array(a) if a.len() <= 5000 => Ok(a),
        _ => Err(Error::Response("invalid member content list")),
    }
}
fn id(v: &Value) -> Result<String> {
    identifier(v)
}

/// Links are selected from the native cached response, never supplied by the
/// WebView. Public official activities open in the OS browser without cookies.
fn link(raw: &str, key: &str, links: &mut BTreeMap<String, String>) -> Option<MemberTarget> {
    for (prefix, album) in [("/playlist/", false), ("/album/", true)] {
        if let Some(value) = raw.strip_prefix(prefix) {
            let id = identifier(&json!(value)).ok()?;
            return Some(if album {
                MemberTarget::Album { id }
            } else {
                MemberTarget::Playlist { id }
            });
        }
    }
    let mut url = url::Url::parse(raw).ok()?;
    if !["http", "https"].contains(&url.scheme())
        || url.host_str() != Some("music.163.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    url.set_scheme("https").ok()?;
    let route = url.fragment().unwrap_or(url.path());
    let route_url = url::Url::parse(&format!("https://music.163.com{route}")).ok()?;
    let id_param = route_url
        .query_pairs()
        .chain(url.query_pairs())
        .find(|(key, _)| key == "id")
        .and_then(|(_, v)| identifier(&json!(v)).ok());
    if let Some(id) = id_param {
        match route_url.path() {
            "/playlist" => return Some(MemberTarget::Playlist { id }),
            "/album" => return Some(MemberTarget::Album { id }),
            "/song" => return Some(MemberTarget::Song { id }),
            "/djradio" => return Some(MemberTarget::Radio { id }),
            _ => (),
        }
    }
    links.insert(key.into(), url.into());
    Some(MemberTarget::External { key: key.into() })
}
pub(super) fn home(value: &Value) -> Result<Projection> {
    let mut links = BTreeMap::new();
    let mut modules = Vec::new();
    for (index, row) in rows(&value["results"])?.iter().enumerate() {
        let key = format!("{index}:{}", row["module"]["moduleType"]);
        let v = &row["value"]["value"];
        let content = match row["value"]["kind"].as_str() {
            Some("rcmdTracks") => MemberContent::Tracks { tracks:rows(v)?.iter().map(mapping::track).collect::<Result<_>>()? },
            Some("topBanner" | "doubleBanners") => {
                let mut cards = Vec::new();
                for (n,banner) in rows(v)?.iter().enumerate() {
                    let card_key = format!("{key}:{n}");
                    let action = if banner["action"]["kind"] == "play" {
                        Some(MemberTarget::Song {id:id(&banner["action"]["resourceId"])?})
                    } else {
                        banner["action"]["linkPath"].as_str().or_else(||banner["jumpUrl"].as_str())
                            .and_then(|url|link(url,&card_key,&mut links))
                    };
                    cards.push(MemberCard {id:card_key,title:text(&banner["title"]),subtitle:String::new(),cover_url:mapping::artwork(&banner["imgUrl"]),action});
                }
                MemberContent::Banners {cards}
            }
            Some("digitAlbums") => MemberContent::Albums {cards:rows(v)?.iter().map(|album|{
                let id=id(&album["albumId"])?;
                Ok(MemberCard {id:id.clone(),title:text(&album["albumName"]),subtitle:text(&album["artistName"]),cover_url:mapping::artwork(&album["cover"]),action:Some(MemberTarget::Album {id})})
            }).collect::<Result<_>>()?},
            Some("playlist") => MemberContent::Playlists {cards:rows(v)?.iter().map(|list|{
                let id=id(&list["id"])?;
                Ok(MemberCard {id:id.clone(),title:text(&list["title"]),subtitle:String::new(),cover_url:mapping::artwork(&list["coverUrl"]),action:Some(MemberTarget::Playlist {id})})
            }).collect::<Result<_>>()?},
            Some("rankList") => MemberContent::Rankings {groups:rows(v)?.iter().map(|group|{
                let mut tracks=Vec::new();
                for row in rows(&group["contentList"])? {
                    let track=if row["track"].is_null() {
                        let mut missing=mapping::track(&json!({"id":id(&row["songId"])?,"name":row["name"],"ar":[{"name":row["artistName"]}],"al":{"picUrl":row["coverUrl"]}}))?;
                        missing.missing=true;missing
                    } else {mapping::track(&row["track"])?};
                    if track.id!=id(&row["songId"])? {return Err(Error::Response("member ranking song identity changed"));}
                    tracks.push(track);
                }
                Ok(MemberRanking {id:id(&group["rankingListId"])?,title:text(&group["rankingListName"]),tracks})
            }).collect::<Result<_>>()?},
            Some("audioBooks") => {
                let mut cards=Vec::new();
                for block in rows(&v["data"]["blockVOS"])? {
                    for item in rows(&block["creatives"])? {
                        let radio=&item["creativeExtInfoVO"]["radio"];
                        let id=id(&radio["id"])?;
                        let ui=&item["uiElement"];
                        cards.push(MemberCard {id:id.clone(),title:text(&ui["mainTitle"]["title"]),subtitle:text(&ui["subTitle"]["title"]),cover_url:mapping::artwork(&ui["image"]["imageUrl"]),action:Some(MemberTarget::Radio {id})});
                    }
                }
                MemberContent::AudioBooks {cards}
            }
            _ => return Err(Error::Response("unsupported stored member module")),
        };
        modules.push(MemberModule {
            key,
            title: text(&row["module"]["showName"]),
            content,
        });
    }
    let carousel = &value["carouselList"];
    let messages = rows(if carousel.is_array() {
        carousel
    } else {
        &carousel["carousels"]
    })?
    .iter()
    .filter_map(|v| v["content"].as_str().map(String::from))
    .collect();
    Ok(Projection {
        page: MemberPage::Home { modules, messages },
        links,
    })
}

pub(super) fn dates(values: &[Value]) -> Result<Vec<String>> {
    if values.len() > 366 {
        return Err(Error::Response("too many recommendation dates"));
    }
    let mut dates = Vec::new();
    for value in values {
        let date = value
            .as_str()
            .ok_or(Error::Response("invalid recommendation date"))?;
        let parts = date.split('-').collect::<Vec<_>>();
        let valid = parts.len() == 3
            && parts[0].len() == 4
            && parts[1].len() == 2
            && parts[2].len() == 2
            && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()));
        if !valid {
            return Err(Error::Response("invalid recommendation date"));
        }
        let month = time::Month::try_from(parts[1].parse::<u8>().unwrap_or(0))
            .map_err(|_| Error::Response("invalid recommendation month"))?;
        time::Date::from_calendar_date(
            parts[0].parse().unwrap_or(0),
            month,
            parts[2].parse().unwrap_or(0),
        )
        .map_err(|_| Error::Response("invalid recommendation day"))?;
        if dates.iter().any(|d| d == date) {
            return Err(Error::Response("duplicate recommendation date"));
        }
        dates.push(date.into());
    }
    Ok(dates)
}
