//! M3149/M3108: one liked-programme presentation visit, independent of full
//! membership and mutation readback. The wire cursor never crosses IPC.
use super::{EpisodeView, MusicClient, PodcastApi, PodcastLibrary, view};
use crate::{
    Error, Result,
    account::SessionToken,
    client::dto::AccountData,
    podcast::{Episode, LikedVoicePage},
    protocol::api::identifier,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
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
    cursor: Option<Cursor>,
    cursors: HashSet<u64>,
    pages: usize,
    capped: bool,
    load: Arc<tokio::sync::Mutex<()>>,
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
            cursor: Some(Cursor {
                wire: json!(0),
                continuation: receipt(),
            }),
            cursors: HashSet::new(),
            pages: 0,
            capped: false,
            load: Arc::default(),
        }
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
        self.count = page.count.or(self.count);
        self.pages += 1;
        self.capped = page.next.is_some() && self.pages == MAX_PAGES;
        self.cursor = page.next.filter(|_| !self.capped).map(|(wire, number)| {
            self.cursors.insert(number);
            Cursor {
                wire,
                continuation: receipt(),
            }
        });
        Ok(self.view(added))
    }
}

#[derive(Default)]
struct Slot {
    owner: Option<(SessionToken, u64)>,
    visit: Option<Visit>,
}

#[derive(Default)]
pub(in crate::client) struct PodcastLikedHost(Mutex<Slot>);

impl MusicClient {
    fn with_liked_visit<T>(
        &self,
        token: &SessionToken,
        operation: impl FnOnce(&mut Option<Visit>) -> Result<T>,
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
            operation(&mut slot.visit)
        })
    }

    pub async fn podcast_liked_page(
        &self,
        epoch: &str,
        query: PodcastLikedQuery,
    ) -> Result<AccountData<PodcastLikedPage>> {
        let token = self.podcast_token(epoch)?;
        let value = match query {
            PodcastLikedQuery::Open {} => self.with_liked_visit(&token, |slot| {
                let visit = Visit::default();
                let view = visit.view(Vec::new());
                *slot = Some(visit);
                Ok(view)
            })?,
            PodcastLikedQuery::Suspend { visit, revision } => {
                self.with_liked_visit(&token, |slot| {
                    let row = slot.as_mut().ok_or(Error::QueueChanged)?;
                    row.current(&visit, &revision)?;
                    row.suspended_at = Some(Instant::now());
                    Ok(row.view(Vec::new()))
                })?
            }
            PodcastLikedQuery::Restore { visit, revision } => {
                self.with_liked_visit(&token, |slot| {
                    let row = slot.as_mut().ok_or(Error::QueueChanged)?;
                    if row.id != visit
                        || row.revision != revision
                        || !row.suspended_at.is_some_and(|at| at.elapsed() < RETAIN_FOR)
                    {
                        return Err(Error::QueueChanged);
                    }
                    row.suspended_at = None;
                    row.revision = receipt();
                    row.load = Arc::default();
                    Ok(row.view(row.rows.iter().map(project_list_row).collect()))
                })?
            }
            PodcastLikedQuery::Next {
                visit,
                revision,
                continuation,
            } => {
                let (cursor, _load) = self.with_liked_visit(&token, |slot| {
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
                self.with_liked_visit(&token, |slot| {
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
        let library = PodcastLibrary::new(self.session.clone())?;
        let observed = library.with_generation_for(&token, Ok)?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                PodcastApi::new(api).liked_count().await
            })
            .await?;
        library.with_generation_for(&token, |current| {
            if observed != current {
                return Err(Error::StaleResponse);
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
        self.with_liked_visit(token, |slot| {
            let row = slot.as_ref().ok_or(Error::QueueChanged)?;
            row.current(&target.visit, &target.revision)?;
            let index = row.indices.get(&id).ok_or(Error::QueueChanged)?;
            Ok(row.rows[*index].clone())
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
}
