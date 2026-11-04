use super::MusicClient;
use crate::{
    Error, Result,
    account::SessionToken,
    client::{
        dto::{AccountData, TrackView},
        mapping,
    },
    membership::Quality,
    playback::{controller::PlaybackCommand, engine::Cancellation},
    protocol::api::identifier,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex, atomic::Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub mod notification;

const LIST: &str = "/api/sub/artist/new/works/song-mv/list/v2";
const PLAY_ALL: &str = "/api/sub/artist/new/works/song/playall";
const LIMIT: usize = 10;
const TRACK_LIMIT: usize = 500;
const LATEST_LIMIT: usize = 50;
const JS_MAX_TIME: u64 = 8_640_000_000_000_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowLatest {
    revision: String,
    tracks: Vec<TrackView>,
}

#[derive(Default)]
pub(in crate::client) struct FollowLatestHost(Arc<Mutex<LatestState>>);

#[derive(Default)]
struct LatestState {
    started: u64,
    loaded: Option<LatestSelection>,
}

struct LatestSelection {
    token: SessionToken,
    revision: String,
    /// The visible result count is independent of the official playable filter.
    playable: Vec<TrackView>,
    first: Option<TrackView>,
    first_playable: bool,
}

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

    pub async fn followed_new_song_latest(&self, epoch: &str) -> Result<AccountData<FollowLatest>> {
        let token = self.playlist_token(epoch)?;
        let request = self.session.with_token(&token, |_| {
            let mut state = self
                .follow_latest
                .0
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            state.started = state
                .started
                .checked_add(1)
                .ok_or(Error::State("latest revision exhausted"))?;
            Ok(state.started)
        })?;
        let (tracks, playable, first, first_playable) = self
            .session
            .run_for(&token, |api, _| async move {
                let response = api.call(PLAY_ALL, json!({}), false, &[200]).await?;
                let Some(songs) = response["data"]["songList"].as_array() else {
                    return Ok((Vec::new(), Vec::new(), None, false));
                };
                if songs.len() > LATEST_LIMIT {
                    return Err(Error::Response("oversized latest followed songs"));
                }
                let first = songs.first().and_then(track);
                let first_playable =
                    songs.first().is_some_and(mapping::online_play_all_eligible) && first.is_some();
                let mut seen = HashSet::new();
                let playable = songs
                    .iter()
                    .filter(|row| mapping::online_play_all_eligible(row))
                    .filter_map(track)
                    .filter(|song| seen.insert(song.id.clone()))
                    .collect();
                Ok((
                    songs.iter().filter_map(track).collect(),
                    playable,
                    first,
                    first_playable,
                ))
            })
            .await?;
        self.session.with_token(&token, |_| {
            let mut state = self
                .follow_latest
                .0
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if state.started != request {
                return Err(Error::StaleResponse);
            }
            let revision = uuid::Uuid::new_v4().simple().to_string();
            state.loaded = Some(LatestSelection {
                token: token.clone(),
                revision: revision.clone(),
                playable,
                first,
                first_playable,
            });
            Ok(AccountData {
                epoch: epoch.into(),
                value: FollowLatest { revision, tracks },
            })
        })
    }

    pub(in crate::client) async fn play_follow_latest(
        &self,
        epoch: &str,
        revision: &str,
        quality: Quality,
    ) -> Result<crate::client::dto::PlayerView> {
        use crate::client::focus::Owner;

        let token = self.playlist_token(epoch)?;
        let unavailable = || Error::State("the latest release's first song is unavailable");
        let (first, first_playable, mut resources) = {
            let state = self
                .follow_latest
                .0
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            let loaded = state
                .loaded
                .as_ref()
                .filter(|s| s.token == token && s.revision == revision)
                .ok_or(Error::StaleResponse)?;
            (
                loaded.first.clone().ok_or_else(unavailable)?,
                loaded.first_playable,
                loaded
                    .playable
                    .iter()
                    .map(|view| {
                        json!({"id":view.id,"level":quality.as_str(),"dt":view.duration_ms,
                        "song":mapping::projected_song_metadata(view)})
                    })
                    .collect::<Vec<_>>(),
            )
        };
        let focus = self.focus.snapshot()?;
        let observed_selection = self.player.selection.load(Ordering::Acquire);
        let observed_fence = self
            .player
            .controller
            .get()
            .map(|controller| controller.snapshot())
            .filter(|state| state.account == token)
            .map(|state| state.fence());
        let first_download = if first_playable {
            None
        } else {
            let (task, path) = self
                .follow_latest_download(epoch, &first.id)
                .await?
                .ok_or_else(unavailable)?;
            let mut head = json!({"id":first.id,"download_task_id":task,
                "path":path,"dt":first.duration_ms,"song":mapping::projected_song_metadata(&first)});
            // Probe rights and file integrity before touching focus or queue.
            // Use the existing actor's resolver when present; no engine needs
            // to be initialized for a failed offline eligibility check.
            use crate::{library::LocalLibrary, playback::source::SourceResolver};
            let local_resolver;
            let resolver = if let Some(controller) = self.player.controller.get() {
                controller.source_resolver()
            } else {
                local_resolver = SourceResolver::new(
                    self.session.clone(),
                    Some(Arc::new(LocalLibrary::new(self.registry.store())?)),
                );
                &local_resolver
            };
            let verified = resolver
                .resolve_follow_latest_download(&head, &token, &first.id)
                .await;
            self.session.check_generation(&token)?;
            let source = match verified {
                Ok(source) => source,
                Err(Error::StaleResponse) => return Err(Error::StaleResponse),
                Err(error)
                    if matches!(
                        error.root(),
                        Error::Io(_)
                            | Error::Media(_)
                            | Error::State(_)
                            | Error::DownloadedFileMissing
                            | Error::MutationBusy
                            | Error::Response("membership has not been refreshed for this account")
                    ) =>
                {
                    return Err(unavailable());
                }
                Err(error) => return Err(error),
            };
            if let Some(quality) = source.actual_quality() {
                head["level"] = quality.into();
            }
            resources.retain(|row| row["id"].as_str() != Some(first.id.as_str()));
            resources.insert(0, head);
            Some(source)
        };
        let controller = self.player.controller().await?;
        let before = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        // Joint playback and FM have their own coordinator. Never detach or
        // issue a room operation as a side effect of this standalone action.
        let _together_gate = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        if before.fm || self.together_attached(&token, None)? {
            return Err(Error::State("latest releases require ordinary playback"));
        }
        let fence = before.fence();
        if observed_fence.is_some_and(|observed| observed != fence) {
            return Err(Error::StaleResponse);
        }
        let (lease, attempt) = self.reserve_prepared_audio(&token, focus, || {
            if self.player.selection.load(Ordering::Acquire) != observed_selection {
                return Err(Error::StaleResponse);
            }
            Ok(self.player.selection.fetch_add(1, Ordering::AcqRel) + 1)
        })?;
        self.finish_media_claim(&token, Owner::Audio, lease).await?;
        let selection = self.player.selection.clone();
        let focus = self.focus.clone();
        let snapshot = self.follow_latest.0.clone();
        let snapshot_token = token.clone();
        let revision = revision.to_owned();
        let ordinary = self.together_detached_guard(&token);
        let value = controller
            .command_guarded(
                &token,
                PlaybackCommand::ReplaceFollowLatest {
                    resources,
                    first_download,
                },
                fence,
                Cancellation::new(move || {
                    selection.load(Ordering::Acquire) == attempt
                        && focus.check(lease, Owner::Audio).is_ok()
                        && !ordinary.is_cancelled()
                        && snapshot.lock().is_ok_and(|state| {
                            state.loaded.as_ref().is_some_and(|s| {
                                s.token == snapshot_token && s.revision == revision
                            })
                        })
                }),
            )
            .await?;
        self.session.check_generation(&token)?;
        super::super::player::project(&value, &token)
    }
}
