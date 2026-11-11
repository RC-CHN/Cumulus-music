use super::{
    MusicClient,
    dto::{AccountData, ArtistReference, TrackView},
    mapping,
};
use crate::{Error, Result, catalog::rankings::*, protocol::api::identifier};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankingView {
    pub fetched_at: u64,
    pub cached: bool,
    pub fresh: bool,
    pub page: RankingPageView,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RankingPageView {
    Home {
        groups: Vec<RankingGroup>,
    },
    AlbumSales {
        year: i32,
        boards: Vec<SalesBoard>,
    },
    SongSales {
        #[serde(rename = "generatedAt")]
        generated_at: Option<u64>,
        songs: Vec<SongSaleView>,
    },
    Album {
        id: String,
        name: String,
        description: String,
        artists: Vec<String>,
        #[serde(rename = "coverUrl")]
        cover_url: Option<String>,
        revision: String,
        tracks: Vec<TrackView>,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongSaleView {
    pub track: TrackView,
    pub quantity: Option<u64>,
    pub indicator: Option<f64>,
    pub rank_incr: Option<i64>,
    pub newest: bool,
}
fn image(value: &mut Option<String>) {
    *value = value.as_ref().and_then(|s| mapping::artwork(&json!(s)));
}
pub(super) fn album_order(id: &str, songs: &[Value]) -> Result<(Vec<Value>, String)> {
    let ids = songs
        .iter()
        .map(|s| identifier(&s["id"]).map(Value::from))
        .collect::<Result<Vec<_>>>()?;
    if ids.len() > 50000 {
        return Err(Error::Response("album too large"));
    }
    let revision = hex::encode(Sha256::digest(serde_json::to_vec(&json!([id, ids]))?));
    Ok((ids, revision))
}
fn project(saved: SavedRanking, cached: bool, choice_hours: Option<f64>) -> Result<RankingView> {
    let fresh = saved.fresh_at_with_choice_hours(
        (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0) as u64,
        choice_hours,
    );
    let page = match saved.page {
        RankingPage::Home { mut groups } => {
            for group in &mut groups {
                for card in &mut group.cards {
                    image(&mut card.cover_url);
                    for track in &mut card.tracks {
                        image(&mut track.cover_url);
                    }
                }
            }
            RankingPageView::Home { groups }
        }
        RankingPage::AlbumSales { year, mut boards } => {
            for board in &mut boards {
                for product in &mut board.products {
                    image(&mut product.cover_url);
                }
            }
            RankingPageView::AlbumSales { year, boards }
        }
        RankingPage::SongSales {
            generated_at,
            songs,
        } => RankingPageView::SongSales {
            generated_at,
            songs: songs
                .into_iter()
                .map(|s| {
                    let track = match s.song {
                        Some(song) => mapping::track(&song)?,
                        None => TrackView {
                            id: s.id,
                            voice_id: None,
                            name: s.name,
                            title_names: Default::default(),
                            artists: vec![s.artist.clone()],
                            artist_refs: vec![ArtistReference {
                                id: None,
                                name: s.artist,
                            }],
                            album: String::new(),
                            album_id: None,
                            cover_url: s
                                .cover_url
                                .as_ref()
                                .and_then(|s| mapping::artwork(&json!(s))),
                            duration_ms: 0,
                            album_popularity: None,
                            fee: 0,
                            missing: true,
                            local: false,
                            playlist_song_comment_menu: None,
                            playlist_song_comment_eligible: false,
                        },
                    };
                    Ok(SongSaleView {
                        track,
                        quantity: s.quantity,
                        indicator: s.indicator,
                        rank_incr: s.rank_incr,
                        newest: s.newest,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        },
        RankingPage::Album { id, album, songs } => {
            let (_, revision) = album_order(&id, &songs)?;
            RankingPageView::Album {
                id,
                name: mapping::string(&album["name"]),
                description: mapping::string(&album["description"]),
                artists: album["artists"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|a| mapping::string(&a["name"]))
                    .collect(),
                cover_url: mapping::artwork(&album["picUrl"]),
                revision,
                tracks: songs
                    .iter()
                    .map(mapping::track)
                    .collect::<Result<Vec<_>>>()?,
            }
        }
    };
    Ok(RankingView {
        fetched_at: saved.fetched_at,
        cached,
        fresh,
        page,
    })
}
impl MusicClient {
    pub async fn ranking_page(
        &self,
        epoch: &str,
        query: RankingQuery,
        refresh: bool,
    ) -> Result<AccountData<RankingView>> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        let service = Rankings::new(self.session.clone())?;
        let saved = if refresh {
            service.page_for(&token, &query).await?
        } else {
            service
                .cached_for(&token, &query)?
                .ok_or(Error::State("ranking page is not cached"))?
        };
        let choice_hours = if matches!(query, RankingQuery::Home) {
            self.auth
                .context
                .as_ref()
                .and_then(|context| context.pc_snapshot().ok().flatten())
                .and_then(|snapshot| snapshot.data)
                .and_then(|data| data["preload#cacheValidateTime"]["choice"]["common"].as_f64())
        } else {
            None
        };
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: project(saved, !refresh, choice_hours)?,
            })
        })
    }
}

#[cfg(test)]
mod tests;
