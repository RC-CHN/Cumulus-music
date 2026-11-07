//! 「包含歌曲」 card and scoped song actions from a bound current episode.
mod actions;
mod collect;
mod download;
mod group_download;
mod likes;
mod picker;
mod purchased;
mod selection;
use super::{
    MusicClient, PlaybackSnapshot, PlaybackStatus, PlayerResourceKind, identity, mapping,
    vinyl_similar::stale_credentials, voice_detail,
};
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::metadata::SongTitleNames,
    client::dto::{ArtistReference, TrackView},
    protocol::{
        api::{VinylVoiceSongsRead, identifier, validate_response},
        wire::serialization::js_truthy,
    },
};
pub(super) use actions::Host;
pub use actions::VoiceSongAction;
use actions::{Page, Row};
pub use collect::{VoiceSongCollectPlan, VoiceSongCollectResult, VoiceSongLikedDestination};
pub use download::{VoiceSongDownloadDecision, VoiceSongDownloadPlan};
pub use group_download::{
    VoiceSongGroupDownloadConfirmation, VoiceSongGroupDownloadDecision, VoiceSongGroupDownloadPlan,
};
pub use picker::{VoiceSongDownloadCreation, VoiceSongDownloadPicker};
pub use purchased::{VoiceSongPurchasedDownloadCommand, VoiceSongPurchasedDownloadResult};
pub use selection::{VoiceSongSelectionAction, VoiceSongSelectionKind};
use serde::Serialize;
use serde_json::Value;
use std::{sync::Arc, time::Instant};

const MAX_TRACKS: usize = 100;
const MAX_ARTISTS: usize = 32;
const MAX_TEXT: usize = 1024;
const MAX_IMAGE_URL: usize = 2048;
const MAX_JS_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerVoiceSongsView {
    pub play_id: String,
    pub voice_id: String,
    pub receipt: Option<String>,
    pub tracks: Vec<TrackView>,
    pub availability: Vec<VoiceSongRowAvailability>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSongRowAvailability {
    pub online_eligible: bool,
    pub copyright_blocked: bool,
    pub group_download: VoiceSongGroupDownloadHint,
}

/// Card-local affordance only; current account, local files and the download
/// ledger are checked by the explicit native prepare/start commands.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSongGroupDownloadHint {
    Ordinary,
    Purchased,
    Omitted,
    Unsupported,
}

#[derive(Clone, PartialEq, Eq)]
struct Seed {
    voice_id: String,
    entry_id: String,
    song_id: String,
    source: EpisodeSource,
}

#[derive(Clone, PartialEq, Eq)]
enum EpisodeSource {
    Voice,
    Scene { id: String, revision: u64 },
    Download { task_id: String },
}

fn seed(snapshot: &PlaybackSnapshot, token: &SessionToken, play_id: &str) -> Result<Seed> {
    if play_id.is_empty()
        || &snapshot.account != token
        || snapshot.play_id.as_deref() != Some(play_id)
    {
        return Err(Error::StaleResponse);
    }
    let entry = snapshot.selected.as_ref().ok_or(Error::StaleResponse)?;
    if snapshot.fm
        || !matches!(
            snapshot.state,
            PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
        )
    {
        return Err(Error::State(
            "vinyl episode songs require a current episode",
        ));
    }
    // Reuse the validated episode/backing-song correspondence; raw nested
    // metadata alone cannot make a local file or song into an episode.
    let (voice_id, song_id, _) = voice_detail::bound_episode(&entry.resource)
        .ok_or(Error::State("invalid current vinyl episode identity"))?;
    let source = match identity::kind(&entry.resource) {
        PlayerResourceKind::Voice if entry.source["kind"] == "scene_fm" => {
            // A new Scene selection can exist while the old decoder is
            // retiring. Only the actor’s committed loaded item is eligible.
            let (scene, voice, revision) = snapshot
                .committed_scene_voice(token)
                .ok_or(Error::StaleResponse)?;
            if voice != voice_id
                || entry.source["scene_id"].as_str() != Some(scene)
                || entry.source["scene_revision"].as_u64() != Some(revision)
            {
                return Err(Error::StaleResponse);
            }
            EpisodeSource::Scene {
                id: scene.into(),
                revision,
            }
        }
        PlayerResourceKind::Voice => EpisodeSource::Voice,
        PlayerResourceKind::Downloaded if entry.source["kind"] != "scene_fm" => {
            // bound_episode already checked the native DownloadOrigin marker
            // and its match with both raw episode and backing song identities.
            let task_id = entry.resource["download_task_id"]
                .as_str()
                .ok_or(Error::StaleResponse)?;
            EpisodeSource::Download {
                task_id: task_id.into(),
            }
        }
        _ => {
            return Err(Error::State(
                "vinyl episode songs require a current episode",
            ));
        }
    };
    Ok(Seed {
        voice_id,
        entry_id: entry.entry_id.clone(),
        song_id,
        source,
    })
}

fn or<'a>(first: &'a Value, fallback: &'a Value) -> &'a Value {
    if js_truthy(first) { first } else { fallback }
}

fn id(value: &Value) -> Option<String> {
    // Avoid cloning arbitrarily long strings from a malformed response.
    if value.as_str().is_some_and(|value| value.len() > 32) {
        return None;
    }
    identifier(value).ok()
}

fn text(value: &Value) -> Result<String> {
    if !js_truthy(value) {
        return Ok(String::new());
    }
    value
        .as_str()
        .filter(|text| text.len() <= MAX_TEXT && !text.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or(Error::Response("invalid vinyl episode song text"))
}

fn credits(row: &Value) -> Result<Vec<ArtistReference>> {
    let artists = or(&row["ar"], &row["artists"]);
    if !js_truthy(artists) {
        return Ok(Vec::new());
    }
    artists
        .as_array()
        .filter(|rows| rows.len() <= MAX_ARTISTS)
        .ok_or(Error::Response("invalid vinyl episode song artists"))?
        .iter()
        .map(|artist| {
            Ok(ArtistReference {
                id: id(&artist["id"]),
                name: text(&artist["name"])?,
            })
        })
        .collect()
}

fn track(row: &Value) -> Result<Option<Row>> {
    if !row.is_object() {
        return Err(Error::Response("invalid vinyl episode song"));
    }
    // M21:D prefers `album ?? al`; a normalized empty album object remains
    // present, while missing/false albums are filtered out by SongCard.
    let album = row
        .get("album")
        .filter(|album| !album.is_null())
        .unwrap_or(&row["al"]);
    if !js_truthy(album) {
        return Ok(None);
    }
    if !album.is_object() {
        return Err(Error::Response("invalid vinyl episode song album"));
    }
    let artist_refs = credits(row)?;
    let image = or(&album["cover"], &album["picUrl"]);
    let track = TrackView {
        id: id(&row["id"]).ok_or(Error::Response("invalid vinyl episode song ID"))?,
        name: text(&row["name"])?,
        title_names: SongTitleNames::from_song(row),
        artists: artist_refs
            .iter()
            .map(|artist| artist.name.clone())
            .collect(),
        artist_refs,
        album: text(or(&album["name"], &album["albumName"]))?,
        album_id: id(or(&album["id"], &album["albumId"])),
        cover_url: image
            .as_str()
            .filter(|url| url.len() <= MAX_IMAGE_URL)
            .and_then(|_| mapping::artwork(image)),
        duration_ms: or(&row["dt"], &row["duration"])
            .as_u64()
            .filter(|duration| *duration <= MAX_JS_INTEGER)
            .unwrap_or(0),
        fee: row["fee"].as_i64().unwrap_or(0),
        voice_id: None,
        missing: false,
        local: false,
    };
    let privilege_id = &row["privilege"]["id"];
    let online_eligible = (privilege_id.is_null()
        || id(privilege_id).as_deref() == Some(track.id.as_str()))
        && mapping::online_play_all_eligible(row);
    // M21 maps raw st to privilege.status; M35.j treats negative status
    // as copyright blocked for the next-play menu, independently of pl.
    let status = &row["privilege"]["st"];
    let copyright_blocked = status
        .as_f64()
        .or_else(|| status.as_str()?.trim().parse().ok())
        .is_some_and(|status| status < 0.0);
    let download = download::classify(row, &track.id);
    Ok(Some(Row {
        download,
        download_copyright_blocked: copyright_blocked
            && !["programId", "audio", "radio", "mainTrackId"]
                .iter()
                .any(|key| js_truthy(&row[*key])),
        group_download_unsupported: group_download::unsupported(row, &track.id, download),
        // M/21 A maps subp to subPriv. M/3085 favorite filters out rows
        // with an explicitly truthy subPriv which is not positive.
        playlist_eligible: !js_truthy(&row["privilege"]["subp"])
            || row["privilege"]["subp"]
                .as_i64()
                .is_some_and(|value| value > 0),
        collection_privilege: collect::Privilege::from_song(row, &track.id),
        track,
        online_eligible,
        copyright_blocked,
    }))
}

fn tracks(reply: Value) -> Result<Vec<Row>> {
    let reply = if reply.is_object() && !js_truthy(&reply["code"]) {
        reply
    } else {
        validate_response(VinylVoiceSongsRead::PATH, reply, &[200])?
    };
    let Some(rows) = reply.get("tracks") else {
        // Undefined uses the default [] in M21:I. An explicit malformed
        // value, including null, cannot be substituted by embedded songs.
        return Ok(Vec::new());
    };
    let rows = rows
        .as_array()
        .filter(|rows| rows.len() <= MAX_TRACKS)
        .ok_or(Error::Response("invalid vinyl episode songs"))?;
    let mut result = Vec::new();
    for row in rows {
        if let Some(track) = track(row)? {
            result.push(track);
        }
    }
    Ok(result)
}

impl MusicClient {
    pub async fn player_voice_songs(
        &self,
        epoch: &str,
        play_id: &str,
    ) -> Result<PlayerVoiceSongsView> {
        let token = self.player_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let initial = seed(&controller.snapshot(), &token, play_id)?;
        let read = VinylVoiceSongsRead::new(&initial.voice_id)?;
        let room = self.together_detached_guard(&token);
        let host = &self.player.voice_songs;
        let check_scope = || {
            self.session
                .with_verified_profile(&token, &api, |_, _| Ok(()))?;
            if self.login_pending()?
                || room.is_cancelled()
                || seed(&controller.snapshot(), &token, play_id).ok().as_ref() != Some(&initial)
            {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        check_scope()?;
        // This is a native credentials snapshot only, not a cohort lookup.
        let revision = api
            .verified_pc_experiment_revision()
            .map_err(stale_credentials)?
            .ok_or(Error::State(
                "vinyl episode songs require signed native PC credentials",
            ))?;
        let check_authority = || {
            if api
                .verified_pc_experiment_revision()
                .map_err(stale_credentials)?
                .as_deref()
                != Some(revision.as_str())
            {
                return Err(Error::StaleResponse);
            }
            check_scope()
        };
        let ticket = host.start(check_authority)?;
        let check_read = || {
            check_authority()?;
            host.reading(ticket)
        };
        let reply = api
            .request_vinyl_voice_songs(read, &revision, &check_read)
            .await;
        check_read()?;
        let rows = tracks(reply.map_err(stale_credentials)?)?;
        let page = Arc::new(Page {
            token: token.clone(),
            play_id: play_id.into(),
            seed: initial.clone(),
            api: Arc::downgrade(&api),
            revision: revision.clone(),
            rows,
            picker: Default::default(),
            collect: Default::default(),
            purchase: Default::default(),
            group_download: Default::default(),
            receipt: uuid::Uuid::new_v4().simple().to_string(),
            created: Instant::now(),
        });
        host.install(ticket, page.clone(), check_authority)?;
        Ok(PlayerVoiceSongsView {
            play_id: page.play_id.clone(),
            voice_id: page.seed.voice_id.clone(),
            receipt: (!page.rows.is_empty()).then(|| page.receipt.clone()),
            tracks: page.rows.iter().map(|row| row.track.clone()).collect(),
            availability: page
                .rows
                .iter()
                .map(|row| VoiceSongRowAvailability {
                    online_eligible: row.online_eligible,
                    copyright_blocked: row.copyright_blocked,
                    group_download: if row.group_download_unsupported {
                        VoiceSongGroupDownloadHint::Unsupported
                    } else {
                        match row.download {
                            download::Class::Ordinary => VoiceSongGroupDownloadHint::Ordinary,
                            download::Class::Purchased(_) => VoiceSongGroupDownloadHint::Purchased,
                            _ => VoiceSongGroupDownloadHint::Omitted,
                        }
                    },
                })
                .collect(),
        })
    }
}
