use super::MusicClient;
use crate::{
    Error, Result,
    client::{
        dto::{AccountData, TrackView},
        mapping,
    },
    protocol::api::identifier,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

const LIST: &str = "/api/sub/artist/new/works/song-mv/list/v2";
const PLAY_ALL: &str = "/api/sub/artist/new/works/song/playall";
const LIMIT: usize = 10;
const TRACK_LIMIT: usize = 500;
const LATEST_LIMIT: usize = 50;
const JS_MAX_TIME: u64 = 8_640_000_000_000_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowNewSongPage {
    releases: Vec<FollowRelease>,
    next_timestamp: Option<u64>,
    latest_visit_time: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FollowRelease {
    kind: FollowReleaseKind,
    publish_time: u64,
    artist_id: String,
    artist_name: String,
    artist_image_url: Option<String>,
    resource_id: Option<String>,
    resource_name: Option<String>,
    resource_image_url: Option<String>,
    album_id: Option<String>,
    album_song_count: Option<u64>,
    tracks: Vec<FollowTrack>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum FollowReleaseKind {
    Song,
    Album,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FollowTrack {
    song: TrackView,
    mv_id: Option<String>,
}

fn timestamp(value: &Value) -> Option<u64> {
    value.as_u64().filter(|n| (1..=JS_MAX_TIME).contains(n))
}

fn now() -> Result<u64> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::State("invalid new song clock"))?
        .as_millis();
    u64::try_from(ms)
        .ok()
        .filter(|n| (1..=JS_MAX_TIME).contains(n))
        .ok_or(Error::State("invalid new song clock"))
}

fn id(value: &Value) -> Option<String> {
    identifier(value).ok().filter(|text| text.len() <= 128)
}

fn text(value: &Value) -> Option<String> {
    let s = value.as_str()?;
    (!s.trim().is_empty() && s.chars().count() <= 200 && !s.chars().any(char::is_control))
        .then(|| s.into())
}

fn track(value: &Value) -> Option<TrackView> {
    // Release resources contain songs. Never treat an unrelated voice_id as
    // an alternative identity for this endpoint.
    let mut song = mapping::track(value).ok()?;
    song.voice_id = None;
    let _ = id(&value["id"])?;
    let _ = text(&value["name"])?;
    song.artists.truncate(32);
    song.artist_refs.truncate(32);
    song.artists.iter_mut().for_each(|s| {
        s.retain(|c| !c.is_control());
        *s = s.chars().take(200).collect();
    });
    song.artist_refs.iter_mut().for_each(|a| {
        a.name.retain(|c| !c.is_control());
        a.name = a.name.chars().take(200).collect();
        a.id = a.id.take().filter(|id| id.len() <= 128);
    });
    song.album.retain(|c| !c.is_control());
    song.album = song.album.chars().take(200).collect();
    song.album_id = song.album_id.take().filter(|id| id.len() <= 128);
    Some(song)
}

fn available_mv(track: &Value) -> Option<String> {
    // M21 maps privilege.st to status. M113 hides the ordinary MV tag
    // for a no-copyright row; the alternate MV requires a separate
    // configCenter experiment and is not inferred from an mvInfo field.
    if track["privilege"]["st"].as_i64().is_some_and(|st| st < 0) {
        return None;
    }
    id(&track["mv"]).or_else(|| id(&track["mvid"]))
}

fn release(info: &Value) -> Option<FollowRelease> {
    let kind = match info["blockType"].as_str()? {
        "song" => FollowReleaseKind::Song,
        "album" => FollowReleaseKind::Album,
        _ => return None,
    };
    let title = &info["blockTitle"];
    let raw_tracks = info["songLists"].as_array()?;
    if raw_tracks.len() > TRACK_LIMIT {
        return None;
    }
    Some(FollowRelease {
        kind,
        publish_time: timestamp(&info["publishTime"])?,
        artist_id: id(&title["artistId"])?,
        artist_name: text(&title["artistName"])?,
        artist_image_url: mapping::artwork(&title["imgUrl"]),
        resource_id: id(&title["resourceId"]),
        resource_name: text(&title["resourceName"]),
        resource_image_url: mapping::artwork(&title["resourcePicUrl"]),
        album_id: id(&info["albumId"]),
        album_song_count: info["albumSongCount"].as_u64().filter(|n| *n <= 100_000),
        tracks: raw_tracks
            .iter()
            .filter_map(|raw| {
                Some(FollowTrack {
                    song: track(raw)?,
                    mv_id: available_mv(raw),
                })
            })
            .collect(),
    })
}

fn project(reply: &Value, start: u64, first: bool) -> Result<FollowNewSongPage> {
    let data = reply["data"]
        .as_object()
        .ok_or(Error::Response("missing followed release data"))?;
    let empty = Vec::new();
    let rows = data
        .get("newWorks")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    if rows.len() > LIMIT {
        return Err(Error::Response("oversized followed release page"));
    }
    let more = data
        .get("hasMore")
        .and_then(Value::as_bool)
        .ok_or(Error::Response("missing followed release continuation"))?;
    let infos = rows
        .iter()
        .filter(|item| item["sourceType"].as_i64() == Some(1))
        .filter_map(|item| item.get("info").filter(|value| !value.is_null()));
    // Official cursor calculation takes the smallest publishTime from all
    // sourceType=1 records, before the renderer filters unsupported blockTypes.
    let end = infos
        .clone()
        .filter_map(|info| timestamp(&info["publishTime"]))
        .fold(start, u64::min);
    if more && !first && end == start {
        return Err(Error::Response("followed release cursor did not advance"));
    }
    Ok(FollowNewSongPage {
        releases: infos.filter_map(release).collect(),
        next_timestamp: more.then_some(end),
        latest_visit_time: first
            .then(|| data.get("latestVisitTime").and_then(timestamp))
            .flatten(),
    })
}

impl MusicClient {
    pub async fn followed_new_song_page(
        &self,
        epoch: &str,
        start_timestamp: Option<u64>,
    ) -> Result<AccountData<FollowNewSongPage>> {
        let token = self.playlist_token(epoch)?;
        let first = start_timestamp.is_none();
        let start = match start_timestamp {
            Some(n) if (1..=JS_MAX_TIME).contains(&n) => n,
            Some(_) => return Err(Error::InvalidInput("invalid followed release cursor")),
            None => now()?,
        };
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                let response = api
                    .call(
                        LIST,
                        json!({
                            "startTimestamp": start, "firstRequest": first,
                            "sourceType": 1, "limit": LIMIT,
                        }),
                        false,
                        &[200],
                    )
                    .await?;
                project(&response, start, first)
            })
            .await?;
        self.session.with_token(&token, |uid| {
            if first && let Ok(store) = self.session.persistent_store() {
                store.put(uid, "follow_new_song", "last_enter_ms", &json!(now()?))?;
            }
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub async fn followed_new_song_latest(
        &self,
        epoch: &str,
    ) -> Result<AccountData<Vec<TrackView>>> {
        let token = self.playlist_token(epoch)?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                let response = api.call(PLAY_ALL, json!({}), false, &[200]).await?;
                let Some(songs) = response["data"]["songList"].as_array() else {
                    return Ok(Vec::new());
                };
                if songs.len() > LATEST_LIMIT {
                    return Err(Error::Response("oversized latest followed songs"));
                }
                Ok(songs.iter().filter_map(track).collect())
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
