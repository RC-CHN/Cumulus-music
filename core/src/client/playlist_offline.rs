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
    account::{AccountSession, SessionToken},
    download::{
        offline::{OFFLINE_PLAYLIST_DOWNLOAD, PlaylistDownload, SearchPublication},
        service::DownloadService,
    },
    library::LocalResource,
    playback::engine::Cancellation,
    playlist::{
        OFFLINE_DELETED_KIND, OFFLINE_SNAPSHOT_KIND, PlaylistLocalRef, PlaylistLocalStore,
        offline_snapshot::OfflinePlaylistFence,
    },
    protocol::api::identifier,
    storage::OperationLock,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

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
    fn identity(&self) -> OfflinePlaylistFence {
        OfflinePlaylistFence {
            id: self.playlist.id.clone(),
            revision: self.local_revision.clone(),
            local_generation: self.local_generation,
            cached_at_ms: self.cached_at_ms,
        }
    }

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
            status: 502..=504,
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

fn snapshot_is_current(
    session: &AccountSession,
    token: &SessionToken,
    identity: &OfflinePlaylistFence,
) -> Result<bool> {
    let store = session.persistent_store()?;
    session.with_token(token, |owner| {
        store.read_snapshot(|tx| identity.matches(tx, owner))
    })
}

pub(super) struct OfflineQueue {
    pub resources: Vec<Value>,
    pub index: usize,
    pub source: Value,
    pub guard: Cancellation,
    pub binding: OfflinePlaylistFence,
    // Directory membership and current index revision must remain fenced
    // until the native actor commits the queue.
    pub _local_lease: Option<OperationLock>,
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
                if tx.get(owner, OFFLINE_DELETED_KIND, id)?.is_some() {
                    return Ok(());
                }
                if let Some(previous) = tx.get(owner, OFFLINE_SNAPSHOT_KIND, id)?
                    && let Ok(previous) = Snapshot::parse(previous, id)
                    && previous
                        .remote_updated_at
                        .zip(snapshot.remote_updated_at)
                        .is_some_and(|(old, new)| old > new)
                {
                    return Ok(());
                }
                tx.put(
                    owner,
                    OFFLINE_SNAPSHOT_KIND,
                    id,
                    &serde_json::to_value(&snapshot)?,
                )
            })
        })
    }

    pub(super) fn invalidate_offline_playlist(&self, token: &SessionToken, id: &str) -> Result<()> {
        let store = self.session.persistent_store()?;
        self.session.with_token(token, |owner| {
            self.playlist_offline.forget(token, id)?;
            if store.get(owner, OFFLINE_SNAPSHOT_KIND, id)?.is_some() {
                store.delete(owner, OFFLINE_SNAPSHOT_KIND, id)?;
            }
            Ok(())
        })
    }

    fn read_offline_playlist(&self, token: &SessionToken, id: &str) -> Result<Option<Snapshot>> {
        identifier(&json!(id))?;
        let store = self.session.persistent_store()?;
        let snapshot = self.session.with_token(token, |owner| {
            store.read_snapshot(|tx| {
                if tx.get(owner, OFFLINE_DELETED_KIND, id)?.is_some() {
                    return Ok(None);
                }
                tx.get(owner, OFFLINE_SNAPSHOT_KIND, id)?
                    .map(|raw| Snapshot::parse(raw, id))
                    .transpose()
            })
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

    async fn offline_playlist_download_candidates(
        &self,
        token: &SessionToken,
        wanted: impl Iterator<Item = String>,
    ) -> Result<HashMap<String, (PlaylistDownload, &'static str)>> {
        let mut ids: Vec<_> = wanted.collect();
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let session = self.session.clone();
        let account = token.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<_> {
            let tasks = DownloadService::published_catalog_songs(&session, &account, &ids)?;
            let mut available = HashMap::new();
            for (song, task) in tasks {
                session.check_generation(&account)?;
                let candidate =
                    SearchPublication::bind(session.clone(), account.clone(), &task, &song)
                        .and_then(|file| Ok((file.playlist_candidate()?, file.quality())));
                match candidate {
                    Ok(value) => {
                        available.insert(song, value);
                    }
                    Err(
                        error @ (Error::StaleResponse
                        | Error::CredentialChanged
                        | Error::Cancelled
                        | Error::LockPoisoned),
                    ) => return Err(error),
                    // A malformed ledger, missing file or unverified right
                    // never makes the catalogue row available.
                    Err(_) => {}
                }
            }
            Ok(available)
        })
        .await
        .map_err(|_| Error::State("offline playlist candidate worker stopped"))??;
        self.session.check_generation(token)?;
        Ok(result)
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
        let downloaded = self
            .offline_playlist_download_candidates(
                token,
                slice.iter().filter_map(|row| match row {
                    Row::Catalog(track) if !track.missing => Some(track.id.clone()),
                    _ => None,
                }),
            )
            .await?;
        let mut download_ids = Vec::new();
        let tracks = slice
            .into_iter()
            .map(|row| match row {
                Row::Catalog(track) => {
                    let mut row = track.as_ref().clone();
                    // Cached metadata alone is never a grant. File hashing is
                    // reserved for the active native selection.
                    row.missing = !downloaded.contains_key(&row.id);
                    if !row.missing && !download_ids.contains(&row.id) {
                        download_ids.push(row.id.clone());
                    }
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
        if !snapshot_is_current(&self.session, token, &snapshot.identity())? {
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
                download_ids,
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
        cached_at_ms: u64,
        start: Option<(&str, bool)>,
        selected_only: bool,
    ) -> Result<OfflineQueue> {
        let snapshot = self
            .read_offline_playlist(token, id)?
            .ok_or(Error::QueueChanged)?;
        if snapshot.local_revision != revision || snapshot.cached_at_ms != cached_at_ms {
            return Err(Error::QueueChanged);
        }
        let binding = snapshot.identity();
        let picked = |id: &str, local: bool| !selected_only || start == Some((id, local));
        let refs = snapshot
            .rows
            .iter()
            .filter_map(|row| match row {
                Row::Local(reference) if picked(&reference.id, true) => Some(reference.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let (mut indexed, local_lease) = if refs.is_empty() {
            (Default::default(), None)
        } else {
            let runtime = self.local.context().await?;
            let (items, lease) =
                tokio::task::spawn_blocking(move || runtime.library.hold_playlist_items(&refs))
                    .await
                    .map_err(|_| Error::State("offline playlist index worker stopped"))??;
            (items, Some(lease))
        };
        let downloaded = self
            .offline_playlist_download_candidates(
                token,
                snapshot.rows.iter().filter_map(|row| match row {
                    Row::Catalog(track) if !track.missing && picked(&track.id, false) => {
                        Some(track.id.clone())
                    }
                    _ => None,
                }),
            )
            .await?;
        let mut resources = Vec::new();
        let mut index = None;
        for row in snapshot.rows {
            let (identity, resource) = match row {
                Row::Local(reference) => {
                    let Some(item) = indexed.remove(&reference.id) else {
                        continue;
                    };
                    let resource = serde_json::to_value(LocalResource {
                        local_id: item.id,
                        local_revision: item.revision,
                        metadata: item.metadata,
                    })?;
                    ((reference.id, true), resource)
                }
                Row::Catalog(track) => {
                    let Some((download, quality)) = downloaded.get(&track.id) else {
                        continue;
                    };
                    let resource = json!({
                        "id": track.id, "download_task_id": download.task_id,
                        OFFLINE_PLAYLIST_DOWNLOAD: download,
                        "_together_catalog_download": true,
                        "song": super::mapping::projected_song_metadata(&track),
                        "dt": track.duration_ms, "level": quality,
                    });
                    ((track.id, false), resource)
                }
                Row::UnboundLocal { .. } => continue,
            };
            if index.is_none() && start.is_none_or(|row| row == (identity.0.as_str(), identity.1)) {
                index = Some(resources.len());
            }
            resources.push(resource);
        }
        let session = self.session.clone();
        let (token, id) = (token.clone(), id.to_owned());
        let source = json!({
            "kind":"playlist", "id":id, "name":snapshot.playlist.name, "offline_playlist":true,
            "history_resource": {"id":id, "name":snapshot.playlist.name},
        });
        let stamp = binding.clone();
        let guard = Cancellation::new(move || {
            snapshot_is_current(&session, &token, &stamp).unwrap_or(false)
        });
        if guard.is_cancelled() {
            return Err(Error::QueueChanged);
        }
        let index = index.ok_or_else(|| {
            if start.is_some() {
                Error::State("cached playlist song is unavailable")
            } else {
                Error::OfflinePlaylistEmpty
            }
        })?;
        Ok(OfflineQueue {
            resources,
            index,
            source,
            guard,
            binding,
            _local_lease: local_lease,
        })
    }
}
