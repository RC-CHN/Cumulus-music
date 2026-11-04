//! Native read contract for the PC queue's "more recommendations" (M/1384,
//! M/15 Bc and Dg). It does not choose when to recommend or change the queue.
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    protocol::{api::identifier, artwork},
};
use serde_json::{Value, json};
use std::sync::Arc;

const RECOMMEND: &str = "/api/homepage/more/rcmd/song";
const PRIVILEGE: &str = "/api/song/enhance/privilege";
const MAX_QUEUE_SEEDS: usize = 10_000; // Official M/7.g queue capacity.
const MAX_BATCH: usize = 3;
const MAX_ID_BYTES: usize = 32;

pub struct QueueMoreSong {
    pub id: String,
    pub name: String,
    pub duration_ms: u64,
    pub fee: Option<u32>,
    pub artists: Vec<super::metadata::ArtistCredit>,
    pub album_id: Option<String>,
    pub album_name: String,
    pub cover_url: Option<String>,
    /// Native correlation from data.algMap. Never submit it with the
    /// privilege read or include it in a renderer projection.
    pub algorithm: Option<String>,
    pub privilege: Option<QueueMorePrivilege>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueMorePrivilege {
    pub status: Option<i64>,
    pub fee: Option<u32>,
    pub paid_flags: Option<u32>,
    pub play_bitrate: Option<u64>,
}

pub struct QueueMoreRead {
    session: Arc<AccountSession>,
}

impl QueueMoreRead {
    pub fn new(session: Arc<AccountSession>) -> Self {
        Self { session }
    }

    /// Accept only the native owner's catalog identities, not paths, opaque
    /// file IDs or renderer-supplied resource metadata. The caller supplies
    /// M/1384's queue order and the current M/308 resource track identity.
    pub async fn for_queue(
        &self,
        owner: &SessionToken,
        song_ids: &[String],
        current_track_id: &str,
    ) -> Result<Vec<QueueMoreSong>> {
        self.session.with_token(owner, |_| Ok(()))?;
        let Some(payload) = request_payload(song_ids, current_track_id)? else {
            return Ok(Vec::new());
        };
        let mut songs = self
            .session
            .run_for(owner, |api, _| async move {
                let response = api.call(RECOMMEND, payload, false, &[200]).await?;
                project_songs(&response["data"])
            })
            .await?;
        let ids: Vec<_> = songs.iter().map(|song| song.id.as_str()).collect();
        let payload = json!({ "ids": serde_json::to_string(&ids)? });
        // A new owner after Bc must not cause even a privilege request with
        // the previous owner's recommendations. Resolve again after Dg, too.
        let privileges = self
            .session
            .run_for(owner, |api, _| async move {
                let response = api.call(PRIVILEGE, payload, false, &[200]).await?;
                response["data"]
                    .as_array()
                    .filter(|rows| rows.len() <= 64)
                    .map(|rows| {
                        rows.iter()
                            .take(MAX_BATCH)
                            .map(project_privilege)
                            .collect::<Vec<_>>()
                    })
                    .ok_or(Error::Response("invalid queue recommendation privileges"))
            })
            .await?;
        for (song, row) in songs.iter_mut().zip(privileges) {
            if let Some((id, privilege)) = row
                && id == song.id
            {
                song.privilege = Some(privilege);
            }
        }
        self.session.with_token(owner, |_| Ok(songs))
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_BYTES
        && id.bytes().all(|value| value.is_ascii_digit())
        && id.bytes().any(|value| value != b'0')
}

fn request_payload(song_ids: &[String], current_track_id: &str) -> Result<Option<Value>> {
    if song_ids.is_empty() {
        return Ok(None);
    }
    if song_ids.len() > MAX_QUEUE_SEEDS
        || song_ids.iter().any(|id| !valid_id(id))
        || (!current_track_id.is_empty() && !valid_id(current_track_id))
    {
        return Err(Error::InvalidInput("invalid queue recommendation identity"));
    }
    Ok(Some(json!({
        "songIds": song_ids,
        "currentPlaySongId": current_track_id,
        "source": "curlist_more_rcmd"
    })))
}

fn safe_id(value: &Value) -> Option<String> {
    identifier(value).ok().filter(|id| valid_id(id))
}

fn bounded_text(value: &Value, max: usize) -> Option<String> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control))
        .map(str::to_owned)
}

fn integer(value: &Value) -> Option<u32> {
    value.as_u64()?.try_into().ok()
}

fn project_songs(data: &Value) -> Result<Vec<QueueMoreSong>> {
    // M/15 Bc formats absent data.songList as an empty list; M/1384
    // nevertheless calls Dg with a serialized empty list.
    let list = match &data["songList"] {
        Value::Null => return Ok(Vec::new()),
        Value::Array(rows) => rows,
        _ => return Err(Error::Response("invalid queue recommendation list")),
    };
    list.iter()
        .take(MAX_BATCH)
        .map(|row| {
            let id = safe_id(&row["id"]).ok_or(Error::Response("invalid recommended song ID"))?;
            let artists = row["ar"].as_array().or_else(|| row["artists"].as_array());
            let artists = artists
                .into_iter()
                .flatten()
                .take(32)
                .map(|row| super::metadata::ArtistCredit {
                    id: safe_id(&row["id"]),
                    name: bounded_text(&row["name"], 512).unwrap_or_default(),
                })
                .collect();
            let album = row["album"]
                .as_object()
                .map(|_| &row["album"])
                .unwrap_or(&row["al"]);
            Ok(QueueMoreSong {
                algorithm: bounded_text(&data["algMap"][&id], 128),
                id,
                name: bounded_text(&row["name"], 4096).unwrap_or_default(),
                duration_ms: row["dt"]
                    .as_u64()
                    .filter(|duration| *duration != 0)
                    .or_else(|| row["duration"].as_u64())
                    .unwrap_or_default(),
                fee: integer(&row["fee"]),
                artists,
                album_id: super::album_identifier(album)
                    .ok()
                    .filter(|id| valid_id(id)),
                album_name: bounded_text(&album["name"], 4096).unwrap_or_default(),
                cover_url: artwork::square(&album["cover"])
                    .or_else(|| artwork::square(&album["picUrl"])),
                // The first response cannot confer rights.
                privilege: None,
            })
        })
        .collect()
}

fn project_privilege(row: &Value) -> Option<(String, QueueMorePrivilege)> {
    Some((
        safe_id(&row["id"])?,
        QueueMorePrivilege {
            status: row["st"].as_i64(),
            fee: integer(&row["fee"]),
            paid_flags: integer(&row["payed"]),
            play_bitrate: row["pl"].as_u64(),
        },
    ))
}
