//! Persist a complete observed order, never a partial page or an audio grant.
use super::{
    MusicClient,
    content::missing_playlist_row,
    dto::{
        PageRequest, PlaylistLocalPage, PlaylistOfflinePage, PlaylistView, TrackPage, TrackView,
    },
    local::view,
};
use crate::{
    Error, Result,
    account::SessionToken,
    library::LocalResource,
    playback::engine::Cancellation,
    playlist::{PlaylistLocalRef, PlaylistLocalStore},
    protocol::api::identifier,
    storage::OperationLock,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const KIND: &str = "playlist-offline-snapshot-v1";
const MAX_ROWS: usize = 50_000;
const MAX_PENDING: usize = 4;

#[derive(Default)]
pub(super) struct Host(Mutex<VecDeque<Pending>>);

struct Pending {
    token: SessionToken,
    total: usize,
    snapshot: Snapshot,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    playlist: PlaylistView,
    remote_revision: String,
    local_revision: String,
    local_generation: Option<u64>,
    remote_updated_at: Option<u64>,
    cached_at_ms: u64,
    rows: Vec<Row>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Row {
    Catalog(Box<TrackView>),
    UnboundLocal { id: String },
    Local(PlaylistLocalRef),
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Snapshot {
    fn parse(value: Value, id: &str) -> Result<Self> {
        let item: Self = serde_json::from_value(value)?;
        if item.playlist.id != id
            || !item.playlist.owned
            || item.playlist.favorite
            || item.playlist.cover_url.is_some()
            || item.playlist.creator_avatar_url.is_some()
            || item.cached_at_ms == 0
            || !digest(&item.remote_revision)
            || !digest(&item.local_revision)
            || item.rows.len() > MAX_ROWS
            || item.rows.iter().any(|row| match row {
                Row::Catalog(track) => {
                    track.cover_url.is_some()
                        || track.local
                        || track.voice_id.is_some()
                        || identifier(&json!(track.id)).is_err()
                }
                Row::Local(reference) => !digest(&reference.id) || !digest(&reference.revision),
                Row::UnboundLocal { id } => {
                    identifier(&json!(id)).is_err()
                        && !(id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
                }
            })
        {
            return Err(Error::State("invalid cached playlist snapshot"));
        }
        Ok(item)
    }
}

impl Host {
    fn observe(
        &self,
        token: &SessionToken,
        mut playlist: PlaylistView,
        page: &TrackPage,
        local_generation: Option<u64>,
        remote_updated_at: Option<u64>,
    ) -> Result<Option<Snapshot>> {
        let (Some(remote_revision), Some(local)) = (&page.revision, &page.playlist_local) else {
            return Ok(None);
        };
        if !playlist.owned || playlist.favorite || page.total > MAX_ROWS {
            return Ok(None);
        }
        let end = page.offset.saturating_add(page.tracks.len());
        if end > page.total
            || page.more != (end < page.total)
            || (page.more && page.tracks.is_empty())
        {
            return Ok(None);
        }
        // Retain display names, never raw service rows, image URLs or any
        // paths from the host. Local metadata is reconstructed from its index.
        playlist.cover_url = None;
        playlist.creator_avatar_url = None;
        let rows = page.tracks.iter().map(|track| {
            if track.local {
                match local.rows.get(&track.id) {
                    Some(revision) => Row::Local(PlaylistLocalRef {
                        id: track.id.clone(),
                        revision: revision.clone(),
                    }),
                    None => Row::UnboundLocal {
                        id: track.id.clone(),
                    },
                }
            } else {
                let mut track = track.clone();
                track.cover_url = None;
                track.voice_id = None;
                Row::Catalog(Box::new(track))
            }
        });
        let mut pending = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        pending.retain(|row| row.token == *token);
        let old = pending
            .iter()
            .position(|row| row.snapshot.playlist.id == playlist.id);
        let mut staged = if page.offset == 0 {
            if let Some(index) = old {
                pending.remove(index);
            }
            Pending {
                token: token.clone(),
                total: page.total,
                snapshot: Snapshot {
                    playlist,
                    remote_revision: remote_revision.clone(),
                    local_revision: local.revision.clone(),
                    local_generation,
                    remote_updated_at,
                    cached_at_ms: 0,
                    rows: Vec::new(),
                },
            }
        } else {
            let Some(index) = old else {
                return Ok(None);
            };
            let source = &pending[index];
            if source.total != page.total
                || source.snapshot.remote_revision != *remote_revision
                || source.snapshot.local_revision != local.revision
                || source.snapshot.local_generation != local_generation
                || source.snapshot.remote_updated_at != remote_updated_at
                || source.snapshot.rows.len() != page.offset
            {
                return Ok(None);
            }
            pending.remove(index).expect("position checked")
        };
        staged.snapshot.rows.extend(rows);
        if page.more {
            if pending.len() == MAX_PENDING {
                pending.pop_front();
            }
            pending.push_back(staged);
            return Ok(None);
        }
        staged.snapshot.cached_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::State("system clock before Unix epoch"))?
            .as_millis()
            .try_into()
            .map_err(|_| Error::State("system clock out of bounds"))?;
        Ok(Some(staged.snapshot))
    }

    fn forget(&self, token: &SessionToken, id: &str) -> Result<()> {
        self.0
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .retain(|row| row.token != *token || row.snapshot.playlist.id != id);
        Ok(())
    }
}

pub(super) fn may_fallback(error: &Error) -> bool {
    match error.root() {
        Error::Transport(_) => true,
        Error::RequestNotSent(cause) => matches!(cause.root(), Error::Transport(_)),
        Error::HttpStatus {
            status: 502 | 503 | 504,
            body,
            ..
        } => body.as_ref().is_none_or(|body| {
            let code = &body["code"];
            code.is_null()
                || code
                    .as_i64()
                    .or_else(|| code.as_str()?.parse().ok())
                    .is_some_and(|value| (500..=599).contains(&value))
        }),
        _ => false,
    }
}

pub(super) fn invalidates_cache(error: &Error) -> bool {
    matches!(
        error.root(),
        Error::HttpStatus {
            status: 401 | 403 | 404 | 410,
            ..
        }
    ) || [401, 403, 404, 410]
        .iter()
        .any(|code| error.is_service_code(*code))
}

pub(super) struct OfflineQueue {
    pub resources: Vec<Value>,
    pub index: usize,
    pub source: Value,
    pub guard: Cancellation,
    // Directory membership and current index revision must remain fenced
    // until the native actor commits the queue.
    pub _local_lease: OperationLock,
}

impl MusicClient {
    pub(super) fn observe_offline_playlist(
        &self,
        token: &SessionToken,
        page: &TrackPage,
        local_generation: Option<u64>,
        remote_updated_at: Option<u64>,
    ) -> Result<()> {
        let Some(playlist) = page.playlist.as_ref() else {
            return Ok(());
        };
        if !playlist.owned || playlist.favorite {
            self.invalidate_offline_playlist(token, &playlist.id)?;
            return Ok(());
        }
        let snapshot = self.session.with_token(token, |_| {
            self.playlist_offline.observe(
                token,
                playlist.clone(),
                page,
                local_generation,
                remote_updated_at,
            )
        })?;
        let Some(snapshot) = snapshot else {
            return Ok(());
        };
        let store = self.session.persistent_store()?;
        let id = &snapshot.playlist.id;
        self.session.with_token(token, |owner| {
            store.transaction(|tx| {
                if let Some(previous) = tx.get(owner, KIND, id)?
                    && let Ok(previous) = Snapshot::parse(previous, id)
                    && previous
                        .remote_updated_at
                        .zip(snapshot.remote_updated_at)
                        .is_some_and(|(old, new)| old > new)
                {
                    return Ok(());
                }
                tx.put(owner, KIND, id, &serde_json::to_value(&snapshot)?)
            })
        })
    }

    pub(super) fn invalidate_offline_playlist(&self, token: &SessionToken, id: &str) -> Result<()> {
        let store = self.session.persistent_store()?;
        self.session.with_token(token, |owner| {
            self.playlist_offline.forget(token, id)?;
            if store.get(owner, KIND, id)?.is_some() {
                store.delete(owner, KIND, id)?;
            }
            Ok(())
        })
    }

    fn read_offline_playlist(&self, token: &SessionToken, id: &str) -> Result<Option<Snapshot>> {
        identifier(&json!(id))?;
        let store = self.session.persistent_store()?;
        let snapshot = self.session.with_token(token, |owner| {
            store
                .get(owner, KIND, id)?
                .map(|raw| Snapshot::parse(raw, id))
                .transpose()
        })?;
        let Some(snapshot) = snapshot else {
            return Ok(None);
        };
        if PlaylistLocalStore::new(self.session.clone())?.generation(token, id)?
            != snapshot.local_generation
        {
            return Ok(None);
        }
        Ok(Some(snapshot))
    }

    pub(super) async fn offline_playlist_page(
        &self,
        token: &SessionToken,
        id: &str,
        page: &PageRequest,
        local_revision: Option<&str>,
    ) -> Result<Option<TrackPage>> {
        let Some(snapshot) = self.read_offline_playlist(token, id)? else {
            return Ok(None);
        };
        if (page.offset > 0 && (page.revision.is_none() || local_revision.is_none()))
            || page
                .revision
                .as_ref()
                .is_some_and(|expected| expected != &snapshot.remote_revision)
            || local_revision.is_some_and(|expected| expected != snapshot.local_revision)
        {
            return Err(Error::QueueChanged);
        }
        let slice = snapshot
            .rows
            .iter()
            .skip(page.offset)
            .take(page.limit)
            .collect::<Vec<_>>();
        let refs = slice
            .iter()
            .filter_map(|row| match row {
                Row::Local(reference) => Some(reference.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let local_rows = refs
            .iter()
            .map(|reference| (reference.id.clone(), reference.revision.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut indexed = if refs.is_empty() {
            Default::default()
        } else {
            let runtime = self.local.context().await?;
            tokio::task::spawn_blocking(move || runtime.library.playlist_local_items(&refs))
                .await
                .map_err(|_| Error::State("offline playlist index worker stopped"))??
        };
        let tracks = slice
            .into_iter()
            .map(|row| match row {
                Row::Catalog(track) => {
                    let mut row = track.as_ref().clone();
                    // Cached metadata is not proof of offline playback rights.
                    row.missing = true;
                    row
                }
                Row::UnboundLocal { id } => missing_playlist_row(id.clone(), true),
                Row::Local(reference) => indexed
                    .remove(&reference.id)
                    .map(|item| view::track(item.id, &item.metadata, false))
                    .unwrap_or_else(|| missing_playlist_row(reference.id.clone(), true)),
            })
            .collect::<Vec<_>>();
        let total = snapshot.rows.len();
        if PlaylistLocalStore::new(self.session.clone())?.generation(token, id)?
            != snapshot.local_generation
        {
            return Err(Error::QueueChanged);
        }
        Ok(Some(TrackPage {
            revision: Some(snapshot.remote_revision),
            more: page.offset.saturating_add(tracks.len()) < total,
            tracks,
            total,
            offset: page.offset,
            playlist: Some(snapshot.playlist),
            playlist_local: Some(PlaylistLocalPage {
                revision: snapshot.local_revision,
                has_members: snapshot.rows.iter().any(|row| matches!(row, Row::Local(_))),
                has_unbound_legacy: snapshot
                    .rows
                    .iter()
                    .any(|row| matches!(row, Row::UnboundLocal { .. })),
                rows: local_rows,
            }),
            playlist_offline: Some(PlaylistOfflinePage {
                cached_at_ms: snapshot.cached_at_ms,
            }),
            daily: None,
            style_tags: None,
        }))
    }

    pub(super) async fn offline_playlist_queue(
        &self,
        token: &SessionToken,
        id: &str,
        revision: &str,
        start: Option<&str>,
        selected_only: bool,
    ) -> Result<OfflineQueue> {
        let snapshot = self
            .read_offline_playlist(token, id)?
            .ok_or(Error::QueueChanged)?;
        if snapshot.local_revision != revision {
            return Err(Error::QueueChanged);
        }
        let refs = snapshot
            .rows
            .into_iter()
            .filter_map(|row| match row {
                Row::Local(reference) if !selected_only || start == Some(reference.id.as_str()) => {
                    Some(reference)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if refs.is_empty() {
            return Err(Error::State("cached playlist has no playable local songs"));
        }
        let runtime = self.local.context().await?;
        let (refs, mut items, local_lease) = tokio::task::spawn_blocking(move || {
            runtime
                .library
                .hold_playlist_items(&refs)
                .map(|(items, lease)| (refs, items, lease))
        })
        .await
        .map_err(|_| Error::State("offline playlist index worker stopped"))??;
        let mut resources = Vec::new();
        let mut index = None;
        for reference in refs {
            let Some(item) = items.remove(&reference.id) else {
                continue;
            };
            if index.is_none() && start.is_none_or(|id| id == item.id) {
                index = Some(resources.len());
            }
            resources.push(serde_json::to_value(LocalResource {
                local_id: item.id,
                local_revision: item.revision,
                metadata: item.metadata,
            })?);
        }
        let index = index.ok_or(Error::State("cached playlist song is unavailable"))?;
        let local = PlaylistLocalStore::new(self.session.clone())?;
        let (token, id) = (token.clone(), id.to_owned());
        let source = json!({
            "kind":"playlist", "id":id, "name":snapshot.playlist.name,
            "history_resource": {"id":id, "name":snapshot.playlist.name},
        });
        let generation = snapshot.local_generation;
        let guard = Cancellation::new(move || {
            local
                .generation(&token, &id)
                .is_ok_and(|current| current == generation)
        });
        if guard.is_cancelled() {
            return Err(Error::QueueChanged);
        }
        Ok(OfflineQueue {
            resources,
            index,
            source,
            guard,
            _local_lease: local_lease,
        })
    }
}
