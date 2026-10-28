use super::{cards, dto::*};
use crate::{
    Error, Result,
    catalog::details::{CatalogQuery, SavedCatalogPage},
    client::{dto::TrackView, mapping},
};
use serde_json::Value;

fn rows(value: &Value) -> Result<&[Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or(Error::Response("missing catalog records"))
}
fn tracks(value: &Value) -> Result<Vec<TrackView>> {
    rows(value)?
        .iter()
        .map(|row| {
            let mut view = mapping::track(row)?;
            if row["name"].as_str().is_none_or(|name| name.is_empty()) {
                view.missing = true;
                view.name = "歌曲资料暂缺".into();
            }
            Ok(view)
        })
        .collect()
}
pub(super) fn page(saved: SavedCatalogPage, cached: bool) -> Result<CatalogView> {
    let value = &saved.response;
    let more = value["more"].as_bool().unwrap_or(false);
    let page = match &saved.query {
        CatalogQuery::Artist { .. } => CatalogPageView::Artist {
            artist: cards::artist(&value["artist"])?,
            tracks: tracks(&value["hotSongs"])?,
            more,
        },
        CatalogQuery::ArtistDynamic { .. } => CatalogPageView::ArtistDynamic {
            followed: value["followed"]
                .as_bool()
                .ok_or(Error::Response("missing artist collection state"))?,
            online_count: value["concert"]["onlineCount"].as_u64().unwrap_or(0),
            video_count: value["videoNum"].as_u64(),
        },
        CatalogQuery::ArtistSongs { offset, .. } => CatalogPageView::ArtistSongs {
            offset: *offset,
            total: value["total"]
                .as_u64()
                .ok_or(Error::Response("missing catalog total"))?,
            more,
            tracks: tracks(&value["songs"])?,
        },
        CatalogQuery::ArtistAlbums { offset, .. } => CatalogPageView::ArtistAlbums {
            offset: *offset,
            total: value["artist"]["albumSize"]
                .as_u64()
                .ok_or(Error::Response("missing catalog total"))?,
            more,
            albums: rows(&value["hotAlbums"])?
                .iter()
                .map(cards::album)
                .collect::<Result<_>>()?,
        },
        CatalogQuery::ArtistIntroduction { .. } => CatalogPageView::ArtistIntroduction {
            brief_description: mapping::string(&value["briefDesc"]),
            sections: rows(&value["introduction"])?
                .iter()
                .map(|row| CatalogIntroduction {
                    title: mapping::string(&row["ti"]),
                    text: mapping::string(&row["txt"]),
                })
                .collect(),
        },
        CatalogQuery::SimilarArtists { offset, .. } => CatalogPageView::SimilarArtists {
            offset: *offset,
            more,
            artists: rows(&value["artists"])?
                .iter()
                .map(cards::artist)
                .collect::<Result<_>>()?,
        },
        CatalogQuery::ArtistVideos { .. } => {
            let data = &value["data"];
            let cursor = &data["page"]["cursor"];
            CatalogPageView::ArtistVideos {
                next_cursor: cursor
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| cursor.to_string()),
                more: data["page"]["more"]
                    .as_bool()
                    .ok_or(Error::Response("missing artist video page"))?,
                size: data["page"]["size"]
                    .as_u64()
                    .ok_or(Error::Response("missing artist video page"))?,
                videos: rows(&data["records"])?
                    .iter()
                    .map(cards::video)
                    .collect::<Result<_>>()?,
            }
        }
        CatalogQuery::ArtistConcerts { offset, limit, .. } => {
            let total = value["total"]
                .as_u64()
                .ok_or(Error::Response("missing catalog total"))?;
            CatalogPageView::ArtistConcerts {
                offset: *offset,
                total,
                more: total > offset.saturating_add(*limit) as u64,
                concerts: rows(&value["simpleConcerts"])?
                    .iter()
                    .map(cards::concert)
                    .collect::<Result<_>>()?,
            }
        }
        CatalogQuery::Album { .. } => CatalogPageView::Album {
            album: cards::album(&value["album"])?,
            read: saved.album_read,
            tracks: tracks(&value["songs"])?,
        },
    };
    Ok(CatalogView {
        query: saved.query,
        fetched_at: saved.fetched_at,
        cached,
        revision: saved.revision,
        page,
    })
}
