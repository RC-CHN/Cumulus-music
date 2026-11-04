use super::ArtistCandidate;
use crate::{
    Error, Result,
    account::artist_onboarding::ArtistFavorScene as Scene,
    catalog::CatalogApi,
    client::{dto::TrackView, mapping},
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};

const MAX_ARTISTS: usize = 250;
const MAX_TRACKS: usize = 100;
pub(super) const SAVE_PATH: &str = "/api/newuser/favor/rcmd-artists/save";

pub(super) fn numeric_id(id: &str) -> Result<u64> {
    let value: u64 = id
        .parse()
        .map_err(|_| Error::InvalidInput("invalid favorite artist ID"))?;
    if value == 0 || value > 9_007_199_254_740_991 || value.to_string() != id {
        return Err(Error::InvalidInput("invalid favorite artist ID"));
    }
    Ok(value)
}

pub(super) fn selection(ids: &[String]) -> Result<Vec<u64>> {
    if !(2..=50).contains(&ids.len()) {
        return Err(Error::InvalidInput("select 2 to 50 artists"));
    }
    let out = ids
        .iter()
        .map(|id| numeric_id(id))
        .collect::<Result<Vec<_>>>()?;
    if out.iter().collect::<HashSet<_>>().len() != out.len() {
        return Err(Error::InvalidInput("duplicate favorite artist"));
    }
    Ok(out)
}

fn candidate(value: &Value) -> Option<ArtistCandidate> {
    let id = identifier(&value["id"])
        .or_else(|_| identifier(&value["artistId"]))
        .ok()?;
    numeric_id(&id).ok()?;
    let name = value["name"]
        .as_str()
        .or_else(|| value["artistName"].as_str())?
        .trim();
    if name.is_empty() {
        return None;
    }
    Some(ArtistCandidate {
        id,
        name: name
            .chars()
            .filter(|ch| !ch.is_control())
            .take(100)
            .collect(),
        cover_url: mapping::artwork(&value["picUrl"])
            .or_else(|| mapping::artwork(&value["img1v1Url"])),
    })
}

fn candidates_from_rows(rows: impl Iterator<Item = Value>) -> Vec<ArtistCandidate> {
    let mut seen = HashSet::new();
    rows.take(MAX_ARTISTS)
        .filter_map(|row| candidate(&row))
        .filter(|row| seen.insert(row.id.clone()))
        .collect()
}

fn arr(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

pub(super) async fn candidates(api: &dyn Api, scene: Scene) -> Result<Vec<ArtistCandidate>> {
    let path = match scene {
        Scene::NewUser => "/api/newuser/favor/rcmd-artists",
        Scene::RecallUser => "/api/recall/user/favor/rcmd-artists",
    };
    let response = api.call(path, json!({}), false, &[200]).await;
    // M/3168 substitutes the official static candidates on a read error or
    // empty response. Let authentication failures reach the session guard.
    let items = match response {
        Ok(value) => match scene {
            Scene::NewUser => candidates_from_rows(arr(&value["data"]).into_iter()),
            Scene::RecallUser => candidates_from_rows(
                arr(&value["data"]["usedListenedArtists"])
                    .into_iter()
                    .chain(arr(&value["data"]["recentHotArtists"])),
            ),
        },
        Err(error) if error.is_service_code(301) => return Err(error),
        Err(_) => Vec::new(),
    };
    if !items.is_empty() {
        return Ok(items);
    }
    let fallback: Vec<Value> = serde_json::from_str(include_str!("fallback.json"))?;
    Ok(candidates_from_rows(fallback.into_iter()))
}

pub(super) async fn similar(api: &dyn Api, scene: Scene, id: u64) -> Result<Vec<ArtistCandidate>> {
    let path = match scene {
        Scene::NewUser => "/api/newuser/favor/similar-rcmd-artists",
        Scene::RecallUser => "/api/recall/user/favor/similar-rcmd-artists",
    };
    let value = api
        .call(path, json!({"artistId":id}), false, &[200])
        .await?;
    let rows = match scene {
        Scene::NewUser => arr(&value["data"]),
        Scene::RecallUser => arr(&value["data"]["similarRcmdArtists"]),
    };
    Ok(candidates_from_rows(rows.into_iter()))
}

pub(super) async fn recommendations(
    api: Arc<dyn Api>,
    scene: Scene,
    ids: &[u64],
) -> Result<Vec<TrackView>> {
    let path = match scene {
        Scene::NewUser => "/api/new/user/favor/artist/song",
        Scene::RecallUser => "/api/recall/user/favor/artist/song",
    };
    let value = api
        .call(path, json!({"artistIds":ids}), false, &[200])
        .await?;
    let songs = match scene {
        Scene::NewUser => arr(&value["data"])
            .into_iter()
            .take(MAX_TRACKS)
            .map(|row| {
                let mut song = row["songData"].clone();
                if let Some(object) = song.as_object_mut() {
                    object.insert("privilege".into(), row["privilege"].clone());
                }
                song
            })
            .collect::<Vec<_>>(),
        Scene::RecallUser => {
            let mut seen = HashSet::new();
            let track_ids = arr(&value["data"]["sortedSongIds"])
                .into_iter()
                .take(MAX_TRACKS)
                .map(|value| identifier(&value).map(Value::from))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .filter(|id| seen.insert(id.clone()))
                .collect::<Vec<_>>();
            if track_ids.is_empty() {
                return Ok(Vec::new());
            }
            CatalogApi::new(api)
                .songs(&track_ids, MAX_TRACKS)
                .await?
                .songs
                .into_iter()
                .flatten()
                .collect()
        }
    };
    let mut seen = HashSet::new();
    Ok(songs
        .iter()
        .map(mapping::track)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|track| seen.insert(track.id.clone()))
        .collect())
}
