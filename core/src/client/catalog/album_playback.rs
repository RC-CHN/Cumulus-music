//! Ordinary album playback distinguishes native cached table rows (M/195,
//! M/3085) from whole resources re-read by M/404 and ordered by M/470.
use super::super::player::origin;
use super::MusicClient;
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::{
        CatalogApi,
        details::{CatalogDetails, CatalogQuery, SavedCatalogPage, album_duration},
    },
    playback::engine::Cancellation,
    protocol::api::identifier,
};
use serde::Deserialize;
use serde_json::{Value, json};

const ALBUM_LIMIT: usize = 50_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlbumDurationOrder {
    #[default]
    Original,
    Ascending,
    Descending,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlbumPlaybackScope {
    pub id: String,
    pub revision: String,
    pub order: AlbumDurationOrder,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlbumPlaybackRow {
    /// Original snapshot ordinals in increasing order; None is the whole table.
    /// The renderer never supplies the sorted playback permutation.
    pub visible_ordinals: Option<Vec<usize>>,
    pub track_index: usize,
    pub track_id: String,
}

fn sorted_ordinals(
    songs: &[Value],
    order: AlbumDurationOrder,
    visible: Option<Vec<usize>>,
) -> Result<Vec<usize>> {
    if songs.len() > ALBUM_LIMIT {
        return Err(Error::InvalidInput("album too large"));
    }
    let mut ordinals = visible.unwrap_or_else(|| (0..songs.len()).collect());
    if ordinals.is_empty() {
        return Err(Error::State("album has no songs"));
    }
    if ordinals.last().is_some_and(|&i| i >= songs.len())
        || ordinals.windows(2).any(|w| w[0] >= w[1])
    {
        return Err(Error::InvalidInput("invalid album row scope"));
    }
    match order {
        AlbumDurationOrder::Original => {}
        AlbumDurationOrder::Ascending => ordinals.sort_by_key(|&i| album_duration(&songs[i])),
        AlbumDurationOrder::Descending => {
            ordinals.sort_by_key(|&i| std::cmp::Reverse(album_duration(&songs[i])));
        }
    }
    Ok(ordinals)
}

fn ordered_ids(songs: &[Value], ordinals: Vec<usize>) -> Result<Vec<Value>> {
    ordinals
        .into_iter()
        .map(|i| identifier(&songs[i]["id"]).map(Value::from))
        .collect()
}

fn cached_album(
    service: &CatalogDetails,
    token: &SessionToken,
    scope: &AlbumPlaybackScope,
) -> Result<SavedCatalogPage> {
    let query = CatalogQuery::Album {
        id: scope.id.clone(),
    };
    let saved = service
        .cached_for(token, &query)?
        .ok_or(Error::QueueChanged)?;
    if saved.album_playback_revision()? != scope.revision {
        return Err(Error::QueueChanged);
    }
    Ok(saved)
}

impl MusicClient {
    pub(crate) fn album_playback_guard(
        &self,
        token: &SessionToken,
        scope: &AlbumPlaybackScope,
    ) -> Result<Cancellation> {
        let service = CatalogDetails::new(self.session.clone())?;
        let query = CatalogQuery::Album {
            id: scope.id.clone(),
        };
        let fence = service.album_playback_fence(token, &query, &scope.revision)?;
        Ok(Cancellation::new(move || fence.is_current()))
    }

    pub(crate) async fn album_playback_selection(
        &self,
        token: &SessionToken,
        scope: &AlbumPlaybackScope,
        row: Option<AlbumPlaybackRow>,
        single_row: bool,
    ) -> Result<(Vec<Value>, usize, Value)> {
        let service = CatalogDetails::new(self.session.clone())?;
        let saved = cached_album(&service, token, scope)?;
        let (ids, index) = if let Some(row) = row {
            let songs = saved.songs().ok_or(Error::QueueChanged)?;
            let anchor = songs.get(row.track_index).ok_or(Error::QueueChanged)?;
            if identifier(&anchor["id"])? != row.track_id
                || anchor["name"].as_str().is_none_or(str::is_empty)
            {
                return Err(Error::QueueChanged);
            }
            let order = sorted_ordinals(songs, scope.order, row.visible_ordinals)?;
            let index = order
                .iter()
                .position(|&i| i == row.track_index)
                .ok_or(Error::QueueChanged)?;
            if single_row {
                (vec![json!(row.track_id)], 0)
            } else {
                (ordered_ids(songs, order)?, index)
            }
        } else {
            let songs = self
                .session
                .run_for(token, |api, _| async {
                    CatalogApi::new(api)
                        .album_playback_tracks(&json!(scope.id))
                        .await
                })
                .await?;
            // The existing original-order receipt still guards replaced or
            // reordered album membership. Durations can legitimately change
            // in the fresh response, and M/470 sorts that response itself.
            saved.verify_song_order(&songs)?;
            let order = sorted_ordinals(&songs, scope.order, None)?;
            let index = order
                .iter()
                .position(|&i| {
                    songs[i]["name"]
                        .as_str()
                        .is_some_and(|name| !name.is_empty())
                })
                .ok_or(Error::State("album has no songs"))?;
            (ordered_ids(&songs, order)?, index)
        };
        Ok((
            ids,
            index,
            origin::named(
                json!({"kind":"album","id":scope.id,"scene":"album","trial_mode":-1}),
                origin::album_name(&saved.response["album"], &scope.id),
            ),
        ))
    }
}
