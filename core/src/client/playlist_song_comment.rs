//! Clicked song menu count for the ordinary, live playlist detail projection.
mod archive;

use super::{MusicClient, dto::*};
use crate::{
    Error, Result,
    account::SessionToken,
    comments::{CommentApi, ResourceKind},
    playlist::PlaylistLocalStore,
    protocol::api::Api,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use uuid::Uuid;

use archive::Archive;

const MAX_ROWS: usize = 50_000;
const COUNT_LIMIT: Duration = Duration::from_millis(1500);

#[derive(Clone, Eq, PartialEq)]
struct LocalFence {
    revision: String,
    generation: Option<u64>,
}

struct Snapshot {
    revision: String,
    local: Option<LocalFence>,
    api: Weak<dyn Api>,
}

impl Snapshot {
    fn same_source(&self, other: &Self) -> bool {
        self.revision == other.revision
            && self.local == other.local
            && Weak::ptr_eq(&self.api, &other.api)
    }
}

#[derive(Clone, Copy)]
enum Ordinal {
    Unobserved,
    Hidden,
    Eligible(Uuid),
}

struct ObservedRow {
    ordinal: usize,
    song: String,
}

struct Visit {
    sequence: u64,
    owner: SessionToken,
    playlist: String,
    snapshot: Option<Snapshot>,
    ordinal: Vec<Ordinal>,
    rows: HashMap<Uuid, ObservedRow>,
}

#[derive(Default)]
struct State {
    sequence: u64,
    visit: Option<Visit>,
    archive: Archive,
}

impl State {
    fn row<'a>(&'a self, visit: &'a Visit, receipt: &Uuid) -> Option<&'a ObservedRow> {
        visit
            .rows
            .get(receipt)
            .or_else(|| self.archive.row(visit, receipt))
    }
}

#[derive(Default)]
pub(super) struct Host(Mutex<State>);

struct BoundRow {
    sequence: u64,
    receipt: Uuid,
    ordinal: usize,
    playlist: String,
    song: String,
    local: Option<LocalFence>,
    api: Arc<dyn Api>,
}

impl Host {
    pub(super) fn begin(&self, owner: &SessionToken, id: &str, page: &PageRequest) -> Option<u64> {
        owner.uid.as_ref()?;
        let mut state = self.0.lock().ok()?;
        if page.offset == 0 {
            let sequence = state.sequence.checked_add(1)?;
            state.sequence = sequence;
            state.archive.retain_owner(owner);
            if let Some(previous) = state.visit.take().filter(|visit| visit.owner == *owner) {
                state.archive.remember(previous);
            }
            state.visit = Some(Visit {
                sequence,
                owner: owner.clone(),
                playlist: id.into(),
                snapshot: None,
                ordinal: Vec::new(),
                rows: HashMap::new(),
            });
            return Some(sequence);
        }
        let visit = state.visit.as_ref()?;
        (visit.owner == *owner
            && visit.playlist == id
            && page.revision.as_ref() == Some(&visit.snapshot.as_ref()?.revision))
        .then_some(visit.sequence)
    }

    pub(super) fn retire(&self, sequence: Option<u64>) {
        if let Some(sequence) = sequence
            && let Ok(mut state) = self.0.lock()
            && state.visit.as_ref().map(|visit| visit.sequence) == Some(sequence)
        {
            state.visit = None;
        }
    }

    pub(super) fn capture_duration(
        &self,
        owner: &SessionToken,
        id: &str,
        source: &PlaylistDurationSource,
        membership_generation: Option<u64>,
        api: &Arc<dyn Api>,
    ) -> Option<u64> {
        if source.cached_at_ms.is_some() {
            return None;
        }
        let state = self.0.lock().ok()?;
        let visit = state.visit.as_ref()?;
        let snapshot = visit.snapshot.as_ref()?;
        let local_matches = match (&snapshot.local, &source.local_revision) {
            (None, None) => true,
            (Some(before), Some(revision)) => {
                before.revision == *revision && before.generation == membership_generation
            }
            _ => false,
        };
        (visit.owner == *owner
            && visit.playlist == id
            && snapshot.revision == source.revision
            && local_matches
            && Weak::ptr_eq(&snapshot.api, &Arc::downgrade(api)))
        .then_some(visit.sequence)
    }

    pub(super) fn observe(
        &self,
        sequence: Option<u64>,
        owner: &SessionToken,
        api: &Arc<dyn Api>,
        membership_generation: Option<u64>,
        page: &mut TrackPage,
    ) {
        self.annotate(sequence, owner, api, membership_generation, page, false);
    }

    pub(super) fn observe_duration(
        &self,
        sequence: u64,
        owner: &SessionToken,
        api: &Arc<dyn Api>,
        membership_generation: Option<u64>,
        page: &mut TrackPage,
    ) {
        // Sorting changes the displayed ordinals, not the canonical order.
        // Existing matching capabilities remain usable if the renderer abandons
        // this result before publication to its visible view.
        self.annotate(
            Some(sequence),
            owner,
            api,
            membership_generation,
            page,
            true,
        );
    }

    fn annotate(
        &self,
        sequence: Option<u64>,
        owner: &SessionToken,
        api: &Arc<dyn Api>,
        membership_generation: Option<u64>,
        page: &mut TrackPage,
        preserve: bool,
    ) {
        let (Some(sequence), Some(playlist), Some(revision), Some(end)) = (
            sequence,
            &page.playlist,
            &page.revision,
            page.offset.checked_add(page.tracks.len()),
        ) else {
            return;
        };
        if page.playlist_offline.is_some() || end > MAX_ROWS {
            return;
        }
        let Ok(mut state) = self.0.lock() else {
            return;
        };
        let Some(visit) = state.visit.as_mut().filter(|visit| {
            visit.sequence == sequence && visit.owner == *owner && visit.playlist == playlist.id
        }) else {
            return;
        };
        let local = page.playlist_local.as_ref().map(|overlay| LocalFence {
            revision: overlay.revision.clone(),
            generation: membership_generation,
        });
        let snapshot = Snapshot {
            revision: revision.clone(),
            local,
            api: Arc::downgrade(api),
        };
        if let Some(previous) = visit.snapshot.as_ref() {
            if !previous.same_source(&snapshot) {
                if !preserve {
                    state.visit = None;
                }
                return;
            }
        } else if page.offset == 0 && !preserve {
            visit.snapshot = Some(snapshot);
        } else {
            return;
        }
        if visit.ordinal.len() < end {
            visit.ordinal.resize(end, Ordinal::Unobserved);
        }
        for (index, track) in page.tracks.iter_mut().enumerate() {
            let index = page.offset + index;
            let ordinal = &mut visit.ordinal[index];
            let eligible = track.playlist_song_comment_eligible
                && !track.local
                && !track.missing
                && track.voice_id.is_none()
                && track.id.len() <= 128;
            let previous = match *ordinal {
                Ordinal::Eligible(receipt) => Some(receipt),
                Ordinal::Hidden | Ordinal::Unobserved => None,
            };
            let reusable = previous.filter(|receipt| {
                preserve
                    && eligible
                    && visit
                        .rows
                        .get(receipt)
                        .is_some_and(|row| row.song == track.id)
            });
            if previous != reusable
                && let Some(old) = previous
            {
                visit.rows.remove(&old);
            }
            let menu = if eligible {
                let receipt = reusable.unwrap_or_else(|| {
                    let receipt = Uuid::new_v4();
                    visit.rows.insert(
                        receipt,
                        ObservedRow {
                            ordinal: index,
                            song: track.id.clone(),
                        },
                    );
                    receipt
                });
                *ordinal = Ordinal::Eligible(receipt);
                PlaylistSongCommentMenu::Eligible {
                    receipt: receipt.simple().to_string(),
                }
            } else {
                *ordinal = Ordinal::Hidden;
                PlaylistSongCommentMenu::Hidden
            };
            track.playlist_song_comment_menu = Some(Box::new(menu));
        }
    }

    fn row(&self, owner: &SessionToken, receipt: &str) -> Result<BoundRow> {
        if receipt.len() != 32
            || !receipt
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::QueueChanged);
        }
        let receipt = Uuid::try_parse(receipt).map_err(|_| Error::QueueChanged)?;
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let visit = state
            .visit
            .as_ref()
            .filter(|visit| visit.owner == *owner)
            .ok_or(Error::QueueChanged)?;
        let snapshot = visit.snapshot.as_ref().ok_or(Error::QueueChanged)?;
        let row = state.row(visit, &receipt).ok_or(Error::QueueChanged)?;
        Ok(BoundRow {
            sequence: visit.sequence,
            receipt,
            ordinal: row.ordinal,
            playlist: visit.playlist.clone(),
            song: row.song.clone(),
            local: snapshot.local.clone(),
            api: snapshot.api.upgrade().ok_or(Error::StaleResponse)?,
        })
    }

    fn check(&self, row: &BoundRow) -> Result<()> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.visit.as_ref().is_some_and(|visit| {
            visit.sequence == row.sequence
                && state.row(visit, &row.receipt).is_some_and(|current| {
                    current.ordinal == row.ordinal && current.song == row.song
                })
        }) {
            Ok(())
        } else {
            Err(Error::QueueChanged)
        }
    }
}

impl MusicClient {
    pub async fn playlist_song_row_comment_count(
        &self,
        epoch: &str,
        receipt: &str,
    ) -> Result<AccountData<Option<u64>>> {
        let owner = self.player_token(epoch)?;
        let row = self.playlist_song_comments.row(&owner, receipt)?;
        let check = || -> Result<()> {
            self.session.require_api(&owner, &row.api)?;
            if let Some(local) = &row.local
                && PlaylistLocalStore::new(self.session.clone())?
                    .generation(&owner, &row.playlist)?
                    != local.generation
            {
                return Err(Error::QueueChanged);
            }
            self.playlist_song_comments.check(&row)
        };
        check()?;
        // M/556 waits for the clicked public Track's count before popup and
        // allows a plain label on failure. Never re-fetch a playlist or guess
        // a Track identity from the renderer; the observed row owns the read.
        let result = tokio::time::timeout(
            COUNT_LIMIT,
            CommentApi::new(row.api.clone()).count(ResourceKind::Track, row.song.clone()),
        )
        .await;
        check()?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: result.ok().and_then(Result::ok),
        })
    }
}
