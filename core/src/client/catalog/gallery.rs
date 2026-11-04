use super::{MusicClient, cards, dto::ArtistCard};
use crate::{Error, Result, client::dto::AccountData};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

const LIMIT: u32 = 20;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtistGalleryQuery {
    area: i16,
    #[serde(rename = "type")]
    kind: i16,
    initial: i16,
    offset: u32,
}

impl ArtistGalleryQuery {
    fn body(&self) -> Result<Value> {
        if !matches!(self.area, -1 | 7 | 96 | 8 | 16 | 0)
            || !matches!(self.kind, -1 | 1..=3)
            || !matches!(self.initial, -1 | 0 | 65..=90)
            || !self.offset.is_multiple_of(LIMIT)
            || self.offset.checked_add(LIMIT).is_none()
        {
            return Err(Error::InvalidInput("invalid artist gallery filter"));
        }
        Ok(json!({
            "area": self.area, "type": self.kind, "initial": self.initial,
            "offset": self.offset, "limit": LIMIT,
        }))
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistGalleryPage {
    artists: Vec<ArtistCard>,
    next_offset: Option<u32>,
}

fn page(value: &Value, offset: u32) -> Result<ArtistGalleryPage> {
    let rows = value["artists"]
        .as_array()
        .filter(|rows| rows.len() <= LIMIT as usize)
        .ok_or(Error::Response("invalid artist gallery rows"))?;
    let more = value["more"]
        .as_bool()
        .ok_or(Error::Response("missing artist gallery continuation"))?;
    let mut seen = HashSet::new();
    let artists = rows
        .iter()
        .filter_map(|row| {
            let mut artist = cards::artist(row).ok()?;
            if artist.name.trim().is_empty()
                || artist.name.chars().count() > 200
                || artist.name.chars().any(char::is_control)
                || !seen.insert(artist.id.clone())
            {
                return None;
            }
            artist.description.clear();
            artist
                .aliases
                .retain(|name| name.chars().count() <= 200 && !name.chars().any(char::is_control));
            artist.aliases.truncate(8);
            Some(artist)
        })
        .collect();
    Ok(ArtistGalleryPage {
        artists,
        next_offset: if more {
            Some(
                offset
                    .checked_add(LIMIT)
                    .ok_or(Error::Response("artist offset overflow"))?,
            )
        } else {
            None
        },
    })
}

impl MusicClient {
    pub async fn artist_gallery(
        &self,
        epoch: &str,
        query: ArtistGalleryQuery,
    ) -> Result<AccountData<ArtistGalleryPage>> {
        let token = self.playlist_token(epoch)?;
        let body = query.body()?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                let reply = api.call("/api/v1/artist/list", body, false, &[200]).await?;
                page(&reply, query.offset)
            })
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_official_filters_preserve_page_width_after_filtered_rows() {
        let query: ArtistGalleryQuery = serde_json::from_value(json!({
            "area": 96, "type": 3, "initial": 65, "offset": 20
        }))
        .unwrap();
        assert_eq!(
            query.body().unwrap(),
            json!({
                "area": 96, "type": 3, "initial": 65, "offset": 20, "limit": 20
            })
        );
        let response = json!({"artists":[{"id":6,"name":"合成歌手", "musicSize":3},
            {"id":"6","name":"重复"},{"id":"7","name":"\t"},{"id":"8","name":"另一位"}],"more":true});
        let result = page(&response, 20).unwrap();
        assert_eq!(
            result
                .artists
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            ["6", "8"]
        );
        assert_eq!(result.artists[0].song_count, 3);
        assert_eq!(result.next_offset, Some(40));
        assert_eq!(page(&response, 180).unwrap().next_offset, Some(200));
        assert_eq!(page(&response, 200).unwrap().next_offset, Some(220));
        assert!(page(&json!({"artists":[]}), 0).is_err());
        assert!(
            ArtistGalleryQuery {
                initial: 97,
                ..query
            }
            .body()
            .is_err()
        );
    }
}
