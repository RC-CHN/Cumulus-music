//! M3149/M3108: one liked-programme presentation visit, independent of full
//! membership and mutation readback. The wire cursor never crosses IPC.
mod checkpoint;

use super::{EpisodeView, MusicClient, PodcastApi, PodcastLibrary, view};
use crate::{
    Error, Result,
    account::SessionToken,
    client::dto::AccountData,
    playback::engine::Cancellation,
    podcast::{Episode, LikedPageQueueFence, LikedVisit, LikedVoicePage, cache_generation_in},
    protocol::api::identifier,
    storage::StateTransaction,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use tokio::time::Instant;

const MAX_PAGES: usize = 500;
const RETAIN_FOR: Duration = Duration::from_secs(60 * 60);

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PodcastLikedQuery {
    /// A new route entry resets the prefix and issues a native first-page
    /// continuation. It makes no request until the mounted list asks for it.
    Open {},
    Next {
        visit: String,
        revision: String,
        continuation: String,
    },
    Suspend {
        visit: String,
        revision: String,
    },
    Restore {
        visit: String,
        revision: String,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PodcastLikedPageTarget {
    pub visit: String,
    pub revision: String,
    pub voice_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastLikedPage {
    pub visit: String,
    pub revision: String,
    /// Open/Restore replace the rendered prefix; Next returns an append only.
    /// Suspend returns no rows.
    pub episodes: Vec<PodcastLikedEpisode>,
    pub loaded_count: usize,
    pub count: Option<u64>,
    /// A Next reply may carry forward a known count; only an explicit
    /// observation in this page supersedes a pending independent count.
    pub count_updated: bool,
    pub continuation: Option<String>,
    pub capped: bool,
}

/// Presentation fields for the common voice table. Native row actions still
/// return EpisodeView and take an opaque, visit-bound target.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastLikedEpisode {
    #[serde(flatten)]
    pub voice: EpisodeView,
    pub listener_count: Option<u64>,
}

fn project_list_row(row: &Episode) -> PodcastLikedEpisode {
    let count = &row.raw["listenerCount"];
    PodcastLikedEpisode {
        voice: project(row),
        listener_count: count.as_u64().or_else(|| count.as_str()?.parse().ok()),
    }
}

fn receipt() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn project(row: &Episode) -> EpisodeView {
    let mut item = view::episode(row, std::slice::from_ref(&row.id));
    if row.display_status_withdrawn() {
        item.withdrawn = Some(true);
        item.unavailable = true;
    }
    item
}

struct Cursor {
    wire: Value,
    continuation: String,
}

struct Visit {
    id: String,
    revision: String,
    suspended_at: Option<Instant>,
    rows: Vec<Episode>,
    indices: HashMap<String, usize>,
    count: Option<u64>,
    count_observation: u64,
    cursor: Option<Cursor>,
    cursors: HashSet<u64>,
    pages: usize,
    capped: bool,
    load: Arc<tokio::sync::Mutex<()>>,
    playback: LikedVisit,
}

impl Default for Visit {
    fn default() -> Self {
        Self {
            id: receipt(),
            revision: receipt(),
            suspended_at: None,
            rows: Vec::new(),
            indices: HashMap::new(),
            count: None,
            count_observation: 0,
            cursor: Some(Cursor {
                wire: json!(0),
                continuation: receipt(),
            }),
            cursors: HashSet::new(),
            pages: 0,
            capped: false,
            load: Arc::default(),
            playback: LikedVisit::default(),
        }
    }
}

impl Drop for Visit {
    fn drop(&mut self) {
        self.playback.retire();
    }
}

impl Visit {
    fn view(&self, episodes: Vec<PodcastLikedEpisode>) -> PodcastLikedPage {
        PodcastLikedPage {
            visit: self.id.clone(),
            revision: self.revision.clone(),
            loaded_count: self.rows.len(),
            episodes,
            count: self.count,
            count_updated: false,
            continuation: self
                .cursor
                .as_ref()
                .filter(|_| self.suspended_at.is_none())
                .map(|cursor| cursor.continuation.clone()),
            capped: self.capped,
        }
    }

    fn current(&self, visit: &str, revision: &str) -> Result<()> {
        if self.suspended_at.is_none() && self.id == visit && self.revision == revision {
            Ok(())
        } else {
            Err(Error::QueueChanged)
        }
    }

    fn program(&self, target: &PodcastLikedPageTarget, id: &str) -> Result<&Episode> {
        self.current(&target.visit, &target.revision)?;
        let index = self.indices.get(id).ok_or(Error::QueueChanged)?;
        Ok(&self.rows[*index])
    }

    fn playable_program(&self, target: &PodcastLikedPageTarget, id: &str) -> Result<&Episode> {
        let row = self.program(target, id)?;
        if row.display_status_withdrawn() || row.raw["grayDisplay"] == true {
            return Err(Error::QueueChanged);
        }
        Ok(row)
    }

    fn cursor(&self, continuation: &str) -> Result<&Cursor> {
        self.cursor
            .as_ref()
            .filter(|cursor| cursor.continuation == continuation)
            .ok_or(Error::QueueChanged)
    }

    fn append(&mut self, page: LikedVoicePage) -> Result<PodcastLikedPage> {
        if page
            .next
            .as_ref()
            .is_some_and(|(_, next)| self.cursors.contains(next))
        {
            return Err(Error::Response("liked voice cursor did not advance"));
        }
        let mut added = Vec::new();
        for row in page.rows {
            if let std::collections::hash_map::Entry::Vacant(entry) =
                self.indices.entry(row.id.clone())
            {
                entry.insert(self.rows.len());
                added.push(project_list_row(&row));
                self.rows.push(row);
            }
        }
        if let Some(count) = page.count {
            self.count = Some(count);
            self.count_observation = self.count_observation.wrapping_add(1);
        }
        self.pages += 1;
        self.capped = page.next.is_some() && self.pages == MAX_PAGES;
        self.cursor = page.next.filter(|_| !self.capped).map(|(wire, number)| {
            self.cursors.insert(number);
            Cursor {
                wire,
                continuation: receipt(),
            }
        });
        let mut view = self.view(added);
        view.count_updated = page.count.is_some();
        Ok(view)
    }
}

#[derive(Default)]
struct Slot {
    owner: Option<(SessionToken, u64)>,
    visit: Option<Visit>,
}

#[derive(Default)]
pub(in crate::client) struct PodcastLikedHost(Mutex<Slot>);

pub(in crate::client) struct LikedPagePlayback {
    pub row: Episode,
    pub prefix: Option<Vec<Episode>>,
    pub authority: Cancellation,
    pub queue_fence: LikedPageQueueFence,
}

/// Held only across a synchronous download insert, never a network await.
/// The caller already holds the current AccountSession lock and the download
/// directory/task locks, and must not re-enter the session while holding this.
pub(in crate::client) struct LikedPageAdmissionLease<'a> {
    _slot: MutexGuard<'a, Slot>,
    generation: u64,
}

impl LikedPageAdmissionLease<'_> {
    pub(in crate::client) fn verify_generation(
        &self,
        tx: &StateTransaction<'_>,
        owner: &str,
    ) -> Result<()> {
        if cache_generation_in(tx, owner)? != self.generation {
            return Err(Error::QueueChanged);
        }
        Ok(())
    }
}

impl MusicClient {
    pub(in crate::client) fn liked_page_download_id(
        &self,
        token: &SessionToken,
        target: &PodcastLikedPageTarget,
    ) -> Result<String> {
        let id = identifier(&json!(target.voice_id))?;
        self.with_liked_visit(token, |slot, _| {
            let row = slot
                .as_ref()
                .ok_or(Error::QueueChanged)?
                .playable_program(target, &id)?;
            Ok(row.id.clone())
        })
    }

    /// Called under the already checked account lock. Keep this separate from
    /// with_liked_visit, which would attempt to lock the account a second time.
    pub(in crate::client) fn lock_liked_page_admission(
        &self,
        token: &SessionToken,
        target: &PodcastLikedPageTarget,
        song_id: &str,
    ) -> Result<LikedPageAdmissionLease<'_>> {
        let id = identifier(&json!(target.voice_id))?;
        let slot = self
            .podcast_liked
            .0
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        let (_, generation) = slot
            .owner
            .as_ref()
            .filter(|(owner, _)| owner == token)
            .ok_or(Error::QueueChanged)?;
        let row = slot
            .visit
            .as_ref()
            .ok_or(Error::QueueChanged)?
            .playable_program(target, &id)?;
        if row.song_id.as_deref().is_some_and(|known| known != song_id) {
            return Err(Error::QueueChanged);
        }
        let generation = *generation;
        Ok(LikedPageAdmissionLease {
            _slot: slot,
            generation,
        })
    }

    fn with_liked_visit<T>(
        &self,
        token: &SessionToken,
        operation: impl FnOnce(&mut Option<Visit>, u64) -> Result<T>,
    ) -> Result<T> {
        PodcastLibrary::new(self.session.clone())?.with_generation_for(token, |generation| {
            let mut slot = self
                .podcast_liked
                .0
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if !slot
                .owner
                .as_ref()
                .is_some_and(|(owner, version)| owner == token && *version == generation)
            {
                slot.owner = Some((token.clone(), generation));
                slot.visit = None;
            }
            operation(&mut slot.visit, generation)
        })
    }

    /// Native clean exit only. Save an active loaded model before pruning
    /// expired presentation rows; neither step exposes a restorable route.
    pub fn finish_podcast_liked_checkpoints(&self) -> Result<usize> {
        let store = self.session.persistent_store()?;
        let save = self.session.token().and_then(|token| {
            let Some(owner) = token.uid.as_deref() else {
                return Ok(());
            };
            self.with_liked_visit(&token, |slot, generation| {
                if let Some(row) = slot.as_ref().filter(|row| row.suspended_at.is_none()) {
                    checkpoint::save(&store, owner, generation, row)?;
                }
                Ok(())
            })
        });
        // Age cleanup is still safe if the optional active snapshot fails.
        let prune = checkpoint::prune(&store);
        save.and(prune)
    }

    fn restore_liked_presentation(
        &self,
        owner: &str,
        generation: u64,
        row: &Visit,
    ) -> Result<Vec<PodcastLikedEpisode>> {
        match self
            .session
            .persistent_store()
            .and_then(|store| checkpoint::take(&store, owner, generation, row))
        {
            Ok(Some(persisted)) => Ok(persisted),
            Err(Error::QueueChanged) => Err(Error::QueueChanged),
            Ok(None) | Err(_) => {
                // A failed or unavailable local cache is optional. Only the
                // already validated same-process suspended model may back POP.
                Ok(row.rows.iter().map(project_list_row).collect())
            }
        }
    }

    pub async fn podcast_liked_page(
        &self,
        epoch: &str,
        query: PodcastLikedQuery,
    ) -> Result<AccountData<PodcastLikedPage>> {
        let token = self.podcast_token(epoch)?;
        let owner = token.uid.as_deref().ok_or(Error::StaleResponse)?;
        let value = match query {
            PodcastLikedQuery::Open {} => self.with_liked_visit(&token, |slot, _| {
                let visit = Visit::default();
                let view = visit.view(Vec::new());
                *slot = Some(visit);
                Ok(view)
            })?,
            PodcastLikedQuery::Suspend { visit, revision } => {
                self.with_liked_visit(&token, |slot, generation| {
                    let row = slot.as_mut().ok_or(Error::QueueChanged)?;
                    row.current(&visit, &revision)?;
                    row.suspended_at = Some(Instant::now());
                    row.playback.retire();
                    // A storage failure must never keep a departed row active.
                    let _ = self
                        .session
                        .persistent_store()
                        .and_then(|store| checkpoint::save(&store, owner, generation, row));
                    Ok(row.view(Vec::new()))
                })?
            }
            PodcastLikedQuery::Restore { visit, revision } => {
                self.with_liked_visit(&token, |slot, generation| {
                    let row = slot.as_mut().ok_or(Error::QueueChanged)?;
                    if row.id != visit
                        || row.revision != revision
                        || row.suspended_at.is_none_or(|at| at.elapsed() >= RETAIN_FOR)
                    {
                        return Err(Error::QueueChanged);
                    }
                    let presentation = self.restore_liked_presentation(owner, generation, row)?;
                    row.suspended_at = None;
                    row.revision = receipt();
                    if let Some(cursor) = &mut row.cursor {
                        cursor.continuation = receipt();
                    }
                    row.load = Arc::default();
                    row.playback = LikedVisit::default();
                    Ok(row.view(presentation))
                })?
            }
            PodcastLikedQuery::Next {
                visit,
                revision,
                continuation,
            } => {
                let (cursor, _load) = self.with_liked_visit(&token, |slot, _| {
                    let row = slot.as_ref().ok_or(Error::QueueChanged)?;
                    row.current(&visit, &revision)?;
                    let cursor = row.cursor(&continuation)?.wire.clone();
                    let guard = row
                        .load
                        .clone()
                        .try_lock_owned()
                        .map_err(|_| Error::MutationBusy)?;
                    Ok((cursor, guard))
                })?;
                let page = self
                    .session
                    .run_for(&token, |api, _| async move {
                        PodcastApi::new(api).liked_voice_page(&cursor).await
                    })
                    .await?;
                self.with_liked_visit(&token, |slot, _| {
                    let row = slot.as_mut().ok_or(Error::QueueChanged)?;
                    row.current(&visit, &revision)?;
                    row.cursor(&continuation)?;
                    row.append(page)
                })?
            }
        };
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    /// The header count has its own endpoint and confers no membership or row
    /// authority. A renderer should ignore replies for an obsolete route.
    pub async fn podcast_liked_count(&self, epoch: &str) -> Result<AccountData<u64>> {
        let token = self.podcast_token(epoch)?;
        let (observed, view) = self.with_liked_visit(&token, |slot, generation| {
            let view = slot
                .as_mut()
                .filter(|visit| visit.suspended_at.is_none())
                .map(|visit| {
                    visit.count_observation = visit.count_observation.wrapping_add(1);
                    (
                        visit.id.clone(),
                        visit.revision.clone(),
                        visit.count_observation,
                    )
                });
            Ok((generation, view))
        })?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                PodcastApi::new(api).liked_count().await
            })
            .await?;
        self.with_liked_visit(&token, |slot, current| {
            if observed != current {
                return Err(Error::StaleResponse);
            }
            if let (Some((id, revision, observation)), Some(visit)) = (view, slot.as_mut())
                && visit.current(&id, &revision).is_ok()
                && visit.count_observation == observation
            {
                visit.count = Some(value);
            }
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    /// Resolve only an observed row in the current active visit. This is not
    /// the legacy full-list receipt, nor a prepared playback/download grant.
    pub fn podcast_liked_page_program(
        &self,
        epoch: &str,
        target: &PodcastLikedPageTarget,
    ) -> Result<AccountData<EpisodeView>> {
        let token = self.podcast_token(epoch)?;
        let row = self.liked_page_row(&token, target)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: project(&row),
        })
    }

    fn liked_page_row(
        &self,
        token: &SessionToken,
        target: &PodcastLikedPageTarget,
    ) -> Result<Episode> {
        let id = identifier(&json!(target.voice_id))?;
        self.with_liked_visit(token, |slot, _| {
            let row = slot.as_ref().ok_or(Error::QueueChanged)?;
            Ok(row.program(target, &id)?.clone())
        })
    }

    pub(in crate::client) fn liked_page_menu_program(
        &self,
        token: &SessionToken,
        target: &PodcastLikedPageTarget,
    ) -> Result<Episode> {
        let row = self.liked_page_row(token, target)?;
        if row.display_status_withdrawn() {
            return Err(Error::QueueChanged);
        }
        Ok(row)
    }

    /// Capture the clicked row and optional currently displayed prefix. The
    /// result never opens another page or the separate membership snapshot.
    pub(in crate::client) fn liked_page_playback(
        &self,
        token: &SessionToken,
        target: &PodcastLikedPageTarget,
        limit: Option<usize>,
    ) -> Result<LikedPagePlayback> {
        let id = identifier(&json!(target.voice_id))?;
        let library = PodcastLibrary::new(self.session.clone())?;
        let (row, prefix, active, generation) =
            self.with_liked_visit(token, |slot, generation| {
                let visit = slot.as_ref().ok_or(Error::QueueChanged)?;
                visit.current(&target.visit, &target.revision)?;
                let row = &visit.rows[*visit.indices.get(&id).ok_or(Error::QueueChanged)?];
                if row.display_status_withdrawn() {
                    return Err(Error::State("liked voice withdrawn"));
                }
                if row.raw["grayDisplay"] == true {
                    return Err(Error::State("liked voice media unavailable"));
                }
                let prefix = limit.map(|limit| {
                    visit
                        .rows
                        .iter()
                        .filter(|voice| {
                            voice.id == id
                                || (voice.song_id.is_some()
                                    && !voice.display_status_withdrawn()
                                    && voice.raw["grayDisplay"] != true)
                        })
                        .take(limit)
                        .cloned()
                        .collect::<Vec<_>>()
                });
                Ok((row.clone(), prefix, visit.playback.clone(), generation))
            })?;
        let queue_fence = active.fence(token, generation);
        let token = token.clone();
        let authority = Cancellation::new(move || {
            active.active()
                && library
                    .with_generation_for(&token, |current| Ok(current == generation))
                    .unwrap_or(false)
        });
        authority.check()?;
        Ok(LikedPagePlayback {
            row,
            prefix,
            authority,
            queue_fence,
        })
    }
}
