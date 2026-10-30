use super::dto::*;
use crate::{
    Result,
    catalog::metadata::{AlbumMetadata, ArtistMetadata, ConcertMetadata, VideoCardMetadata},
    client::{dto::ArtistReference, mapping},
};
use serde_json::{Value, json};

pub(in crate::client) fn artist(value: &Value) -> Result<ArtistCard> {
    let artist = ArtistMetadata::from_official(value)?;
    let aliases = artist.display_aliases();
    Ok(ArtistCard {
        cover_url: mapping::artwork(&json!(artist.avatar_url))
            .or_else(|| mapping::artwork(&json!(artist.picture_url))),
        id: artist.id,
        name: artist.name,
        aliases,
        description: artist.brief_description,
        song_count: artist.song_count,
        album_count: artist.album_count,
        mv_count: artist.mv_count,
    })
}
pub(in crate::client) fn album(value: &Value) -> Result<AlbumCard> {
    let album = AlbumMetadata::from_official(value)?;
    let primary_artist = album.primary_artist.as_ref().map(|a| ArtistReference {
        id: a.id.clone(),
        name: a.name.clone(),
    });
    let credits = if album.artists.is_empty() {
        album.primary_artist.into_iter().collect()
    } else {
        album.artists
    };
    Ok(AlbumCard {
        id: album.id,
        name: album.name,
        aliases: album.aliases,
        translations: album.translations,
        description: album.description,
        primary_artist,
        artists: credits
            .into_iter()
            .map(|a| ArtistReference {
                id: a.id,
                name: a.name,
            })
            .collect(),
        cover_url: mapping::artwork(&json!(album.cover_url)),
        track_count: album.track_count,
        published_at: album.published_at,
        subscribed: album.subscribed,
        explicit: album.explicit,
    })
}
pub(in crate::client) fn video(value: &Value) -> Result<CatalogVideoCard> {
    let video = VideoCardMetadata::from_official(value)?;
    Ok(CatalogVideoCard {
        id: video.id,
        kind: video.kind,
        title: video.title,
        description: video.description,
        artists: video
            .artists
            .into_iter()
            .map(|a| ArtistReference {
                id: a.id,
                name: a.name,
            })
            .collect(),
        cover_url: mapping::artwork(&json!(video.cover_url)),
        duration_ms: video.duration_ms,
        play_count: video.play_count,
        mv_id: video.mv_id,
    })
}
pub(super) fn concert(value: &Value) -> Result<ConcertCard> {
    let concert = ConcertMetadata::from_official(value)?;
    Ok(ConcertCard {
        id: concert.id,
        title: concert.title,
        cover_url: mapping::artwork(&json!(concert.cover_url)),
        start: concert.start,
        end: concert.end,
        address: concert.address,
        price: concert.price,
    })
}
