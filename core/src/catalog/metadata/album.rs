use super::{ArtistCredit, first, strings, text};
use crate::Result;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
pub struct AlbumMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub aliases: Vec<String>,
    pub translations: Vec<String>,
    pub primary_artist: Option<ArtistCredit>,
    pub artists: Vec<ArtistCredit>,
    pub cover_url: String,
    pub cover_id: Option<String>,
    pub track_count: u64,
    pub subscribed_count: u64,
    pub subscribed: Option<bool>,
    pub published_at: Option<i64>,
    pub explicit: bool,
}
impl AlbumMetadata {
    pub fn from_official(value: &Value) -> Result<Self> {
        let mut primary_artist = value["artist"]
            .is_object()
            .then(|| ArtistCredit::from_official(&value["artist"]));
        if let Some(name) = value["artistName"].as_str().filter(|name| !name.is_empty()) {
            primary_artist = Some(ArtistCredit {
                id: None,
                name: name.into(),
            });
        }
        let translations = first(value, &["tns", "transNames"]);
        let translations = if translations.is_null() {
            value["transName"]
                .as_str()
                .filter(|name| !name.is_empty())
                .map(|name| vec![name.to_owned()])
                .unwrap_or_default()
        } else {
            strings(translations)
        };
        let cover_id = if super::truthy(first(value, &["pic_str", "pic"])) {
            let id = first(value, &["pic_str", "pic", "picId"]);
            crate::protocol::api::identifier(id).ok()
        } else {
            None
        };
        Ok(Self {
            id: crate::catalog::album_identifier(value)?,
            name: text(first(value, &["name", "albumName"])),
            description: text(&value["description"]),
            aliases: strings(first(value, &["alia", "ala", "alias"])),
            translations,
            primary_artist,
            artists: value["artists"]
                .as_array()
                .into_iter()
                .flatten()
                .map(ArtistCredit::from_official)
                .collect(),
            cover_url: text(first(value, &["cover", "picUrl"])),
            cover_id,
            track_count: first(value, &["trackCount", "size"]).as_u64().unwrap_or(0),
            subscribed_count: first(value, &["subscribedCount", "subCount"])
                .as_u64()
                .unwrap_or(0),
            subscribed: value["isSub"].as_bool(),
            published_at: value["publishTime"].as_i64(),
            explicit: value["mark"]
                .as_i64()
                .map(|mark| mark as u64)
                .or_else(|| value["mark"].as_u64())
                .is_some_and(|mark| mark & (1 << 20) != 0),
        })
    }
}
