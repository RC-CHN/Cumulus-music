use super::CatalogQuery;
use crate::{
    Error, Result,
    catalog::{
        AlbumReadState, ArtistApi, ArtistPageRequest, CatalogApi, privileges::merge_song_privileges,
    },
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use std::sync::Arc;

fn rows(value: &Value) -> Result<&Vec<Value>> {
    let rows = value
        .as_array()
        .ok_or(Error::Response("missing catalog records"))?;
    if rows.iter().any(|row| !row.is_object()) {
        return Err(Error::Response("invalid catalog record"));
    }
    Ok(rows)
}
fn identified_rows(value: &Value) -> Result<()> {
    for row in rows(value)? {
        identifier(&row["id"])?;
    }
    Ok(())
}
fn identity(value: &Value, id: &str) -> Result<()> {
    if identifier(&value["id"])? != id {
        return Err(Error::Response("catalog identity mismatch"));
    }
    Ok(())
}
fn count(value: &Value) -> Result<()> {
    value
        .as_u64()
        .ok_or(Error::Response("missing catalog total"))?;
    Ok(())
}
pub(super) fn validate(query: &CatalogQuery, response: &Value) -> Result<()> {
    match query {
        CatalogQuery::Artist { id } => {
            identity(&response["artist"], id)?;
            identified_rows(&response["hotSongs"])?;
        }
        CatalogQuery::ArtistDynamic { .. } => {
            response["followed"]
                .as_bool()
                .ok_or(Error::Response("missing artist collection state"))?;
            if !response["concert"]["onlineCount"].is_null() {
                count(&response["concert"]["onlineCount"])?;
            }
        }
        CatalogQuery::ArtistSongs { .. } => {
            identified_rows(&response["songs"])?;
            count(&response["total"])?;
        }
        CatalogQuery::ArtistAlbums { id, .. } => {
            for album in rows(&response["hotAlbums"])? {
                crate::catalog::album_identifier(album)?;
            }
            count(&response["artist"]["albumSize"])?;
            if !response["artist"]["id"].is_null() {
                identity(&response["artist"], id)?;
            }
        }
        CatalogQuery::ArtistIntroduction { .. } => {
            rows(&response["introduction"])?;
        }
        CatalogQuery::SimilarArtists { .. } => {
            identified_rows(&response["artists"])?;
        }
        CatalogQuery::ArtistVideos {
            cursor: previous, ..
        } => {
            for row in rows(&response["data"]["records"])? {
                crate::catalog::metadata::VideoCardMetadata::from_official(row)?;
            }
            let page = &response["data"]["page"];
            if !page.is_object() || !page["more"].is_boolean() || page["size"].as_u64().is_none() {
                return Err(Error::Response("missing artist video page"));
            }
            let cursor = &page["cursor"];
            let text = cursor
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| cursor.to_string());
            let next = crate::catalog::artist::video_cursor(&text)
                .map_err(|_| Error::Response("invalid artist video next cursor"))?;
            if page["more"] == true
                && (page["size"] == 0 || next == crate::catalog::artist::video_cursor(previous)?)
            {
                return Err(Error::Response("artist video pagination did not advance"));
            }
        }
        CatalogQuery::ArtistConcerts { .. } => {
            for row in rows(&response["simpleConcerts"])? {
                crate::catalog::metadata::ConcertMetadata::from_official(row)?;
            }
            count(&response["total"])?;
        }
        CatalogQuery::Album { id } => {
            if crate::catalog::album_identifier(&response["album"])? != *id {
                return Err(Error::Response("album identity mismatch"));
            }
            identified_rows(&response["songs"])?;
        }
    }
    if !response["more"].is_null() && !response["more"].is_boolean() {
        return Err(Error::Response("invalid catalog continuation flag"));
    }
    Ok(())
}

pub(super) async fn fetch(
    api: Arc<dyn Api>,
    query: &CatalogQuery,
) -> Result<(Value, Option<AlbumReadState>)> {
    query.validate()?;
    let artist = ArtistApi::new(api.clone());
    let catalog = CatalogApi::new(api);
    let id = json!(query.id());
    let mut album_read = None;
    let mut response = match query {
        CatalogQuery::Artist { .. } => artist.detail(&id).await?,
        CatalogQuery::ArtistDynamic { .. } => artist.dynamic(&id).await?,
        CatalogQuery::ArtistSongs { offset, limit, .. } => {
            artist
                .songs(
                    &id,
                    ArtistPageRequest {
                        offset: *offset,
                        limit: *limit,
                    },
                )
                .await?
        }
        CatalogQuery::ArtistAlbums { offset, limit, .. } => {
            artist
                .albums(
                    &id,
                    ArtistPageRequest {
                        offset: *offset,
                        limit: *limit,
                    },
                )
                .await?
        }
        CatalogQuery::ArtistIntroduction { .. } => artist.introduction(&id).await?,
        CatalogQuery::SimilarArtists { offset, limit, .. } => {
            artist
                .similar(
                    &id,
                    ArtistPageRequest {
                        offset: *offset,
                        limit: *limit,
                    },
                )
                .await?
        }
        CatalogQuery::ArtistVideos { cursor, size, .. } => {
            artist.videos(&id, cursor, *size).await?
        }
        CatalogQuery::ArtistConcerts { offset, limit, .. } => {
            artist
                .concerts(
                    &id,
                    ArtistPageRequest {
                        offset: *offset,
                        limit: *limit,
                    },
                )
                .await?
        }
        CatalogQuery::Album { .. } => {
            let result = catalog.album_details(&id).await?;
            album_read = Some(result.read);
            result.response
        }
    };
    validate(query, &response)?;
    if matches!(query, CatalogQuery::Artist { .. }) {
        let ids = response["hotSongs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| identifier(&row["id"]))
            .collect::<Result<Vec<_>>>()?;
        let privileges = catalog.privileges(&ids, None).await?;
        response["hotSongs"] = json!(merge_song_privileges(
            response["hotSongs"].as_array().unwrap(),
            &privileges
        )?);
    }
    Ok((response, album_read))
}
