//! M/3111 Album TrackViewer → M/3085 splice. The displayed album rows
//! already exist natively; no album, song or playback request on drop.
use super::{
    AlbumDropRow, Error, MusicClient, QUEUE_LIMIT, Quality, Result, SessionToken, Value,
    identifier, js_truthy, json, mapping, queue_playlist::queue_resource_for_track,
};
use crate::catalog::details::{CatalogDetails, CatalogQuery};
use crate::client::player::origin;

pub(super) fn resolve(
    client: &MusicClient,
    token: &SessionToken,
    id: &str,
    revision: &str,
    rows: Vec<AlbumDropRow>,
    quality: Quality,
) -> Result<(Vec<Value>, Value)> {
    if rows.is_empty() || rows.len() > QUEUE_LIMIT {
        return Err(Error::InvalidInput("invalid album drop selection size"));
    }
    let id = identifier(&json!(id))?;
    let query = CatalogQuery::Album { id: id.clone() };
    let saved = CatalogDetails::new(client.session.clone())?
        .cached_for(token, &query)?
        .filter(|saved| saved.revision == revision)
        .ok_or(Error::AlbumDropSourceChanged)?;
    let songs = saved
        .songs()
        .ok_or(Error::Response("missing album drop tracks"))?;
    if songs.len() > 50_000 {
        return Err(Error::InvalidInput(
            "album drop source exceeds the read bound",
        ));
    }
    let mut resources = Vec::with_capacity(rows.len());
    for row in rows {
        let song = songs
            .get(row.track_index)
            .filter(|song| identifier(&song["id"]).is_ok_and(|id| id == row.track_id))
            .ok_or(Error::InvalidInput(
                "album drop row is outside the verified source",
            ))?;
        if song["name"].as_str().is_none_or(|name| name.is_empty())
            || song["v"].as_i64() == Some(-1)
            || !song["local_id"].is_null()
            || !song["cloud_song_id"].is_null()
            || !song["voice_id"].is_null()
            || js_truthy(&song["privilege"]["cs"])
        {
            return Err(Error::InvalidInput(
                "album drop row is not an ordinary song",
            ));
        }
        resources.push(queue_resource_for_track(&mapping::track(song)?, quality));
    }
    Ok((
        resources,
        origin::named(
            json!({"kind":"album","id":id,"scene":"album"}),
            origin::album_name(&saved.response["album"], &id),
        ),
    ))
}
