//! Durable identity of a complete, account-owned playlist cache observation.
use super::{OFFLINE_DELETED_KIND, PlaylistLocalStore};
use crate::{Result, storage::StateTransaction};

#[derive(Clone)]
pub(crate) struct OfflinePlaylistFence {
    pub id: String,
    pub revision: String,
    pub local_generation: Option<u64>,
    pub cached_at_ms: u64,
}

impl OfflinePlaylistFence {
    pub(crate) fn matches(&self, tx: &StateTransaction<'_>, owner: &str) -> Result<bool> {
        Ok(tx.get(owner, OFFLINE_DELETED_KIND, &self.id)?.is_none()
            && tx.offline_playlist_snapshot_matches(
                owner,
                &self.id,
                &self.revision,
                self.cached_at_ms,
            )?
            && PlaylistLocalStore::generation_in(tx, owner, &self.id)? == self.local_generation)
    }
}

pub(crate) enum PlaylistQueueBinding {
    Offline(OfflinePlaylistFence),
    Online {
        id: String,
        membership: Option<Option<u64>>,
    },
}

impl PlaylistQueueBinding {
    pub(crate) fn matches(&self, tx: &StateTransaction<'_>, owner: &str) -> Result<bool> {
        match self {
            Self::Offline(stamp) => stamp.matches(tx, owner),
            Self::Online { id, membership } => match membership {
                Some(expected) => {
                    Ok(PlaylistLocalStore::generation_in(tx, owner, id)? == *expected)
                }
                None => Ok(true),
            },
        }
    }
}
