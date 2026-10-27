use super::*;
use crate::{
    Error, Result,
    catalog::CatalogApi,
    protocol::api::{Api, validate_response},
};
use serde_json::json;
use std::sync::Arc;

pub struct RankingsApi {
    api: Arc<dyn Api>,
}
impl RankingsApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }
    pub async fn page(&self, query: &RankingQuery) -> Result<RankingPage> {
        query.validate()?;
        match query {
            RankingQuery::Home => {
                let (main, albums, songs) = tokio::try_join!(
                    self.api
                        .call("/api/toplist/detail/v2", json!({}), false, &[200]),
                    self.api.call(
                        "/api/feealbum/songsaleboard/daily/type",
                        json!({"limit":3,"albumType":0}),
                        false,
                        &[200]
                    ),
                    self.api
                        .call("/api/single/sales-ranking/get", json!({}), false, &[200]),
                )?;
                normalize::home(&main, &albums, &songs)
            }
            RankingQuery::AlbumSales { year } => self.album_sales(*year, false).await,
            RankingQuery::AlbumYear { year } => self.album_sales(*year, true).await,
            RankingQuery::SongSales => {
                let response = self
                    .api
                    .call("/api/single/sales-ranking/get", json!({}), false, &[200])
                    .await?;
                let mut songs = normalize::song_sales(&response)?;
                let ids = songs.iter().map(|s| json!(s.id)).collect::<Vec<_>>();
                let details = CatalogApi::new(self.api.clone()).songs(&ids, 100).await?;
                for (song, detail) in songs.iter_mut().zip(details.songs) {
                    song.song = detail;
                }
                Ok(RankingPage::SongSales {
                    generated_at: response["data"]["genTime"].as_u64(),
                    songs,
                })
            }
            RankingQuery::Album { id } => {
                let response = CatalogApi::new(self.api.clone()).album(&json!(id)).await?;
                if !response["album"].is_object() {
                    return Err(Error::Response("missing album"));
                }
                if crate::protocol::api::identifier(&response["album"]["id"])? != *id {
                    return Err(Error::Response("album identity mismatch"));
                }
                let songs = response["songs"]
                    .as_array()
                    .ok_or(Error::Response("missing album songs"))?
                    .clone();
                Ok(RankingPage::Album {
                    id: id.clone(),
                    album: response["album"].clone(),
                    songs,
                })
            }
        }
    }
    async fn album_sales(&self, year: i32, historical: bool) -> Result<RankingPage> {
        let periods: &[&str] = if historical {
            &["year"]
        } else {
            &["daily", "week", "total", "year"]
        };
        let mut payload = json!({});
        let mut children = Vec::new();
        for period in periods {
            for singles in [false, true] {
                // M3140 uses a trailing slash to distinguish the two batch keys;
                // M48 serializes each child object as JSON text.
                let path = format!(
                    "/api/feealbum/songsaleboard/{period}/type{}",
                    if singles { "/" } else { "" }
                );
                let mut args = json!({"albumType":u8::from(singles),"limit":50});
                if *period == "year" {
                    args["year"] = json!(year);
                }
                payload[&path] = serde_json::to_string(&args)?.into();
                children.push((path, period, singles));
            }
        }
        let response = self.api.call("/api/batch", payload, false, &[200]).await?;
        let mut boards = Vec::new();
        for (path, period, singles) in children {
            let child = validate_response(&path, response[&path].clone(), &[200])?;
            boards.push(SalesBoard {
                period: (*period).into(),
                year: (*period == "year").then_some(year),
                singles,
                products: normalize::products(&child)?,
            });
        }
        Ok(RankingPage::AlbumSales { year, boards })
    }
}
