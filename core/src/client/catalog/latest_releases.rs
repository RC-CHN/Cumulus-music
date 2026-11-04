//! Global new releases from the recovered PC models M/3146 and M/3147.
use super::{MusicClient, cards, dto::AlbumCard};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    client::{
        dto::{AccountData, TrackView},
        mapping,
    },
};
use chrono::{Datelike, Local};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashSet, VecDeque},
    sync::Mutex,
};

#[derive(Clone, Copy, Deserialize)]
pub enum NewSongArea {
    #[serde(rename = "0")]
    All,
    #[serde(rename = "7")]
    Chinese,
    #[serde(rename = "96")]
    Western,
    #[serde(rename = "16")]
    Korean,
    #[serde(rename = "8")]
    Japanese,
    #[serde(rename = "536870911")]
    Other,
}

impl NewSongArea {
    fn id(self) -> &'static str {
        match self {
            Self::All => "0",
            Self::Chinese => "7",
            Self::Western => "96",
            Self::Korean => "16",
            Self::Japanese => "8",
            Self::Other => "536870911",
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum NewAlbumArea {
    All,
    Zh,
    Ea,
    Kr,
    Jp,
    Other,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewAlbumCursor {
    year: i32,
    month: u32,
    offset: usize,
}

impl NewAlbumCursor {
    fn resolve(input: Option<Self>) -> Result<Self> {
        let now = Local::now();
        let cursor = input.unwrap_or(Self {
            year: now.year(),
            month: now.month(),
            offset: 0,
        });
        if !(1970..=now.year()).contains(&cursor.year)
            || !(1..=12).contains(&cursor.month)
            || (cursor.year == now.year() && cursor.month > now.month())
            || cursor.offset > 100_000
        {
            return Err(Error::InvalidInput("invalid new album cursor"));
        }
        Ok(cursor)
    }

    fn previous(self) -> Option<Self> {
        let (year, month) = if self.month == 1 {
            (self.year - 1, 12)
        } else {
            (self.year, self.month - 1)
        };
        (year >= 1970).then_some(Self {
            year,
            month,
            offset: 0,
        })
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LatestReleaseQuery {
    Songs {
        area: NewSongArea,
    },
    Albums {
        area: NewAlbumArea,
        recommended: bool,
        cursor: Option<NewAlbumCursor>,
    },
}

#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LatestReleasePage {
    Songs {
        tracks: Vec<TrackView>,
        playable_ids: Vec<String>,
        revision: String,
    },
    Albums {
        year: i32,
        month: u32,
        week: Vec<AlbumCard>,
        albums: Vec<AlbumCard>,
        next: Option<NewAlbumCursor>,
    },
}

fn tracks(response: &Value, revision: String) -> Result<LatestReleasePage> {
    let rows = response["data"]
        .as_array()
        .filter(|rows| rows.len() <= 100)
        .ok_or(Error::Response("invalid new song list"))?;
    let mut seen = HashSet::new();
    let mut playable_ids = Vec::new();
    let tracks = rows
        .iter()
        .filter_map(|row| {
            let mut track = mapping::track(row).ok()?;
            if track.name.trim().is_empty() || !seen.insert(track.id.clone()) {
                return None;
            }
            // M/21 accepts older long-form records and nested privilege.
            track.fee = row["privilege"]["fee"].as_i64().unwrap_or(track.fee);
            if mapping::online_play_all_eligible(row) {
                playable_ids.push(track.id.clone());
            }
            Some(track)
        })
        .collect();
    Ok(LatestReleasePage::Songs {
        tracks,
        playable_ids,
        revision,
    })
}

const MAX_WEEK_ALBUMS: usize = 2048;
const MAX_MONTH_CHUNK: usize = 512;

fn append_album_cards(rows: &[Value], seen: &mut HashSet<String>, cards_out: &mut Vec<AlbumCard>) {
    cards_out.extend(rows.iter().filter_map(|row| {
        let mut album = cards::album(row).ok()?;
        if album.name.trim().is_empty() || !seen.insert(album.id.clone()) {
            return None;
        }
        album.description.clear();
        Some(album)
    }));
}

struct AlbumChunk {
    month: Vec<Value>,
    week: Vec<Value>,
    more: bool,
}

fn album_chunk(response: Value) -> Result<AlbumChunk> {
    let Value::Object(mut object) = response else {
        return Err(Error::Response("invalid new album response"));
    };
    let month = match object.remove("monthData") {
        Some(Value::Array(rows)) if rows.len() <= MAX_MONTH_CHUNK => rows,
        _ => return Err(Error::Response("invalid new album month list")),
    };
    let week = match object.remove("weekData") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(rows)) if rows.len() <= MAX_WEEK_ALBUMS => rows,
        _ => return Err(Error::Response("invalid new album week list")),
    };
    let more = object
        .get("hasMore")
        .and_then(Value::as_bool)
        .ok_or(Error::Response("missing new album continuation"))?;
    Ok(AlbumChunk { month, week, more })
}

async fn album_page(
    session: &AccountSession,
    token: &SessionToken,
    area: NewAlbumArea,
    recommended: bool,
    mut cursor: NewAlbumCursor,
) -> Result<LatestReleasePage> {
    let limit = if recommended { 10 } else { 50 };
    let mut month = Vec::new();
    let mut week = Vec::new();
    let mut month_seen = HashSet::new();
    let mut week_seen = HashSet::new();
    // M/3147 drains the whole month only in all-album mode. Each underlying
    // request is independently fenced so an old account cannot fetch the tail.
    for _ in 0..32 {
        let response = session
            .run_for(token, |api, _| async move {
                api.call(
                    "/api/discovery/new/albums/area",
                    json!({"area":area,"year":cursor.year,"month":cursor.month,
                    "offset":cursor.offset,"limit":limit,"rcmd":recommended}),
                    false,
                    &[200],
                )
                .await
            })
            .await?;
        let chunk = album_chunk(response)?;
        let next = if chunk.more {
            let offset = cursor
                .offset
                .checked_add(chunk.month.len())
                .filter(|&n| n > cursor.offset && n <= 100_000)
                .ok_or(Error::Response("new album cursor did not advance"))?;
            Some(NewAlbumCursor { offset, ..cursor })
        } else {
            cursor.previous()
        };
        append_album_cards(&chunk.month, &mut month_seen, &mut month);
        append_album_cards(&chunk.week, &mut week_seen, &mut week);
        if week.len() > MAX_WEEK_ALBUMS {
            return Err(Error::Response("too many new weekly albums"));
        }
        if recommended || !chunk.more {
            return Ok(LatestReleasePage::Albums {
                year: cursor.year,
                month: cursor.month,
                week,
                albums: month,
                next,
            });
        }
        cursor = next.ok_or(Error::Response("missing same-month cursor"))?;
    }
    Err(Error::Response("too many new album pages in one month"))
}

struct RowsReceipt {
    token: SessionToken,
    area: &'static str,
    revision: String,
    playable_ids: Vec<String>,
}

#[derive(Default)]
pub(in crate::client) struct LatestReleasesHost(Mutex<VecDeque<RowsReceipt>>);

impl MusicClient {
    pub(in crate::client) fn latest_release_rows_for(
        &self,
        token: &SessionToken,
        area: NewSongArea,
        revision: &str,
        start_id: &str,
    ) -> Result<(Vec<String>, usize)> {
        self.session.with_token(token, |_| {
            let receipts = self
                .latest_releases
                .0
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            let receipt = receipts
                .iter()
                .find(|row| {
                    &row.token == token && row.area == area.id() && row.revision == revision
                })
                .ok_or(Error::State("latest song list no longer available"))?;
            let index = receipt
                .playable_ids
                .iter()
                .position(|id| id == start_id)
                .ok_or(Error::InvalidInput(
                    "song is not in the playable release list",
                ))?;
            Ok((receipt.playable_ids.clone(), index))
        })
    }
    pub async fn latest_releases(
        &self,
        epoch: &str,
        query: LatestReleaseQuery,
    ) -> Result<AccountData<LatestReleasePage>> {
        let token = self.playlist_token(epoch)?;
        let song_area = match &query {
            LatestReleaseQuery::Songs { area } => Some(*area),
            _ => None,
        };
        let value = match query {
            LatestReleaseQuery::Songs { area } => {
                self.session
                    .run_for(&token, |api, _| async move {
                        let response = api
                            .call(
                                "/api/v2/discovery/new/songs",
                                json!({"areaId":area.id(),"limit":100,"offset":0}),
                                false,
                                &[200],
                            )
                            .await?;
                        tracks(&response, uuid::Uuid::new_v4().simple().to_string())
                    })
                    .await?
            }
            LatestReleaseQuery::Albums {
                area,
                recommended,
                cursor,
            } => {
                album_page(
                    &self.session,
                    &token,
                    area,
                    recommended,
                    NewAlbumCursor::resolve(cursor)?,
                )
                .await?
            }
        };
        self.session.with_token(&token, |_| {
            if let (
                Some(area),
                LatestReleasePage::Songs {
                    revision,
                    playable_ids,
                    ..
                },
            ) = (song_area, &value)
            {
                let mut receipts = self
                    .latest_releases
                    .0
                    .lock()
                    .map_err(|_| Error::LockPoisoned)?;
                receipts.retain(|row| row.token == token);
                if receipts.len() == 32 {
                    receipts.pop_front();
                }
                receipts.push_back(RowsReceipt {
                    token: token.clone(),
                    area: area.id(),
                    revision: revision.clone(),
                    playable_ids: playable_ids.clone(),
                });
            }
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
