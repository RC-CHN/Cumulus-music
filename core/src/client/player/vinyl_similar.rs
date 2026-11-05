//! Current-song "相似歌曲" in the standard vinyl t1 lane (M/182, M/253).
use super::{
    MusicClient, PlaybackCommand, PlaybackSnapshot, PlaybackStatus, PlayerResourceKind, PlayerView,
    identity, mapping, project, queue_playlist::insert_collection_bound,
};
use crate::{
    Error, Result,
    account::SessionToken,
    client::{dto::TrackView, focus::Owner},
    membership::Quality,
    playback::{
        controller::{QueueTransport, QueueUpdate},
        engine::Cancellation,
    },
    protocol::api::{Api, VinylSimilarSongsRead, identifier, validate_response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const EXPERIMENT: &str = "PH-PC-playPage-recommendMore-new";
const PATH: &str = "/api/v1/discovery/simiSong";
const MAX_ROWS: usize = 100;
const RECEIPT_LIFETIME: Duration = Duration::from_secs(600);

#[derive(Clone, Copy, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VinylSimilarAction {
    Header,
    Cover { index: usize },
    DoubleClick { index: usize },
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VinylSimilarSongView {
    pub track: TrackView,
    pub playable: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VinylSimilarSongsView {
    pub play_id: String,
    pub song_id: String,
    pub receipt: Option<String>,
    pub songs: Vec<VinylSimilarSongView>,
}

struct Page {
    receipt: String,
    ticket: u64,
    token: SessionToken,
    play_id: String,
    song_id: String,
    api: Weak<dyn Api>,
    revision: String,
    created: Instant,
    songs: Vec<VinylSimilarSongView>,
}

#[derive(Default)]
pub(super) struct Host {
    ticket: AtomicU64,
    page: Mutex<Option<Arc<Page>>>,
}
impl Host {
    fn start(&self) -> Result<u64> {
        self.ticket
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                old.checked_add(1)
            })
            .map(|old| old + 1)
            .map_err(|_| Error::State("vinyl similar songs request revision exhausted"))
    }

    fn current(&self, page: &Page) -> bool {
        self.ticket.load(Ordering::Acquire) == page.ticket
            && page.created.elapsed() <= RECEIPT_LIFETIME
    }

    fn get(&self, token: &SessionToken, play_id: &str, receipt: &str) -> Result<Arc<Page>> {
        if receipt.len() != 32 || !receipt.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::StaleResponse);
        }
        self.page
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .filter(|page| {
                &page.token == token
                    && page.play_id == play_id
                    && page.receipt == receipt
                    && self.current(page)
            })
            .cloned()
            .ok_or(Error::StaleResponse)
    }

    fn put(&self, page: Arc<Page>) -> Result<()> {
        let mut active = self.page.lock().map_err(|_| Error::LockPoisoned)?;
        if !self.current(&page) {
            return Err(Error::StaleResponse);
        }
        *active = (!page.songs.is_empty()).then_some(page);
        Ok(())
    }
}

fn seed(snapshot: &PlaybackSnapshot, token: &SessionToken, play_id: &str) -> Result<String> {
    if &snapshot.account != token || snapshot.play_id.as_deref() != Some(play_id) {
        return Err(Error::StaleResponse);
    }
    let resource = &snapshot
        .selected
        .as_ref()
        .ok_or(Error::State("vinyl similar songs require current music"))?
        .resource;
    if snapshot.fm || identity::kind(resource) != PlayerResourceKind::Catalog {
        return Err(Error::State(
            "vinyl similar songs require current catalog music",
        ));
    }
    identifier(&resource["id"])
}

fn rows(value: &Value) -> Result<Vec<VinylSimilarSongView>> {
    let songs = value["songs"]
        .as_array()
        .filter(|songs| songs.len() <= MAX_ROWS)
        .ok_or(Error::Response("invalid vinyl similar songs"))?;
    songs
        .iter()
        .map(|song| {
            let track = mapping::track(song)?;
            VinylSimilarSongsRead::new(&track.id)?;
            if track.voice_id.is_some() || !song["voice"].is_null() || !song["local_id"].is_null() {
                return Err(Error::Response("invalid vinyl similar song type"));
            }
            Ok(VinylSimilarSongView {
                track,
                playable: mapping::online_play_all_eligible(song),
            })
        })
        .collect()
}

fn resource(song: &VinylSimilarSongView, quality: Quality) -> Value {
    json!({"id":song.track.id,"level":quality.as_str(),"dt":song.track.duration_ms,
        "missing":false,"song":mapping::projected_song_metadata(&song.track)})
}

fn stale_credentials(error: Error) -> Error {
    if matches!(error.root(), Error::CredentialChanged) {
        Error::StaleResponse
    } else {
        error
    }
}

impl MusicClient {
    pub async fn player_vinyl_similar_available(&self, epoch: &str) -> Result<bool> {
        Ok(self
            .verified_pc_experiment_groups(epoch)
            .await?
            .group(EXPERIMENT)
            == Some("t1"))
    }

    pub async fn player_vinyl_similar_songs(
        &self,
        epoch: &str,
        play_id: &str,
    ) -> Result<VinylSimilarSongsView> {
        let token = self.player_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let first = controller.snapshot();
        let song_id = seed(&first, &token, play_id)?;
        let read = VinylSimilarSongsRead::new(&song_id)?;
        let ticket = self.player.vinyl_similar.start()?;
        let (revision, groups) = self
            .verified_pc_experiment_groups_for(&token, &api)
            .await
            .map_err(stale_credentials)?;
        if groups.group(EXPERIMENT) != Some("t1") {
            return Err(Error::State(
                "vinyl similar songs experiment is unavailable",
            ));
        }
        let check = || {
            self.session.require_api(&token, &api)?;
            if self.login_pending()?
                || api
                    .verified_pc_experiment_revision()
                    .map_err(stale_credentials)?
                    .as_deref()
                    != Some(revision.as_str())
                || seed(&controller.snapshot(), &token, play_id)? != song_id
                || self.player.vinyl_similar.ticket.load(Ordering::Acquire) != ticket
            {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        check()?;
        let reply = api
            .request_vinyl_similar_songs(read, &revision)
            .await
            .map_err(stale_credentials)?;
        check()?;
        let songs = rows(&validate_response(PATH, reply, &[200])?)?;
        let page = Arc::new(Page {
            receipt: uuid::Uuid::new_v4().simple().to_string(),
            ticket,
            token: token.clone(),
            play_id: play_id.into(),
            song_id: song_id.clone(),
            api: Arc::downgrade(&api),
            revision: revision.clone(),
            created: Instant::now(),
            songs,
        });
        check()?;
        self.player.vinyl_similar.put(page.clone())?;
        Ok(VinylSimilarSongsView {
            play_id: page.play_id.clone(),
            song_id: page.song_id.clone(),
            receipt: (!page.songs.is_empty()).then(|| page.receipt.clone()),
            songs: page.songs.clone(),
        })
    }

    pub async fn player_vinyl_similar_play(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
        action: VinylSimilarAction,
        quality: Quality,
    ) -> Result<PlayerView> {
        let token = self.player_token(epoch)?;
        let page = self.player.vinyl_similar.get(&token, play_id, receipt)?;
        let api = page.api.upgrade().ok_or(Error::StaleResponse)?;
        let (revision, groups) = self
            .verified_pc_experiment_groups_for(&token, &api)
            .await
            .map_err(stale_credentials)?;
        if revision != page.revision || groups.group(EXPERIMENT) != Some("t1") {
            return Err(Error::StaleResponse);
        }
        let check = || {
            self.session.require_api(&token, &api)?;
            if self.login_pending()?
                || !self.player.vinyl_similar.current(&page)
                || api
                    .verified_pc_experiment_revision()
                    .map_err(stale_credentials)?
                    .as_deref()
                    != Some(page.revision.as_str())
            {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        check()?;
        let song =
            page.songs
                .get(match action {
                    VinylSimilarAction::Header => 0,
                    VinylSimilarAction::Cover { index }
                    | VinylSimilarAction::DoubleClick { index } => index,
                })
                .ok_or(Error::InvalidInput("invalid vinyl similar-song row"))?;
        if !song.playable {
            return Err(Error::State("vinyl similar song is unavailable"));
        }

        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let selection = self.player.selection.clone();
        let observed = selection.load(Ordering::Acquire);
        let focus = self.focus.snapshot()?;
        let _room = self
            .together
            .gate
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        let (before, queue) = controller.queue_with_state(&token).await?;
        if seed(&before, &token, play_id)? != page.song_id {
            return Err(Error::StaleResponse);
        }
        if queue.intelligence.is_some() || self.together_attached(&token, None)? {
            return Err(Error::State(
                "vinyl similar songs require an ordinary queue",
            ));
        }
        let is_double = matches!(action, VinylSimilarAction::DoubleClick { .. });
        let fence = before.fence();
        let command = if is_double
            && song.track.id == page.song_id
            && matches!(
                before.state,
                PlaybackStatus::Preparing
                    | PlaybackStatus::Loading
                    | PlaybackStatus::Playing
                    | PlaybackStatus::Paused
                    | PlaybackStatus::Buffering
            ) {
            PlaybackCommand::Pause(!before.paused)
        } else {
            let replace = is_double && self.replace_queue_on_double_click(&token)?;
            let source = json!({"kind":"vinyl_similar","id":page.song_id,"text":"相似歌曲",
                "scene":if matches!(action, VinylSimilarAction::Header) { "vinyl" } else { "unknown" }});
            if replace || matches!(action, VinylSimilarAction::Header) {
                let mut seen = HashSet::new();
                let resources: Vec<_> = page
                    .songs
                    .iter()
                    .filter(|row| row.playable && seen.insert(row.track.id.clone()))
                    .map(|row| resource(row, quality))
                    .collect();
                if replace {
                    let index = resources
                        .iter()
                        .position(|row| row["id"] == song.track.id)
                        .ok_or(Error::State("vinyl similar song was not selected"))?;
                    PlaybackCommand::Replace {
                        resources,
                        index,
                        source,
                    }
                } else {
                    let mut queue =
                        insert_collection_bound(queue, resources, source, false, true, true);
                    queue.current = Some(selected_entry(&queue, &song.track.id)?);
                    PlaybackCommand::UpdateQueue(QueueUpdate {
                        expected: fence.clone(),
                        queue,
                        transport: Some(QueueTransport {
                            position: None,
                            paused: false,
                        }),
                    })
                }
            } else if song.track.id == page.song_id {
                PlaybackCommand::Play
            } else {
                let mut queue = insert_collection_bound(
                    queue,
                    vec![resource(song, quality)],
                    source,
                    false,
                    true,
                    true,
                );
                queue.current = Some(selected_entry(&queue, &song.track.id)?);
                PlaybackCommand::UpdateQueue(QueueUpdate {
                    expected: fence.clone(),
                    queue,
                    transport: Some(QueueTransport {
                        position: None,
                        paused: false,
                    }),
                })
            }
        };
        check()?;
        let attempt = observed
            .checked_add(1)
            .ok_or(Error::State("playback selection exhausted"))?;
        selection
            .compare_exchange(observed, attempt, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::StaleResponse)?;
        let lease = if focus.owner == Some(Owner::Audio) {
            focus.generation
        } else {
            self.claim_native_media(&token, Owner::Audio, focus).await?
        };
        check()?;
        let focus = self.focus.clone();
        let host = self.player.vinyl_similar.clone();
        let session = self.session.clone();
        let captured = token.clone();
        let result = controller
            .command_guarded(
                &token,
                command,
                fence,
                Cancellation::new(move || {
                    selection.load(Ordering::Acquire) == attempt
                        && focus.check(lease, Owner::Audio).is_ok()
                        && host.current(&page)
                        && session.require_api(&captured, &api).is_ok()
                        && api.verified_pc_experiment_revision().is_ok_and(|revision| {
                            revision.as_deref() == Some(page.revision.as_str())
                        })
                }),
            )
            .await?;
        self.session.check_generation(&token)?;
        project(&result, &token)
    }
}

fn selected_entry(queue: &crate::playback::QueueSnapshot, id: &str) -> Result<String> {
    Ok(queue
        .items
        .iter()
        .find(|row| identity::online_song(&row.resource, id))
        .ok_or(Error::State("vinyl similar song was not inserted"))?
        .entry_id
        .clone())
}
