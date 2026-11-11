//! Recently displaced live observations, eligible only under a fresh matching visit.
use super::{MAX_ROWS, ObservedRow, Ordinal, Snapshot, Visit};
use crate::account::SessionToken;
use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
};
use tokio::time::Instant;
use uuid::Uuid;

const MAX_VISITS: usize = 8;
const MAX_ARCHIVED_ROWS: usize = 2 * MAX_ROWS;
const LIFETIME: Duration = Duration::from_secs(30 * 60);

struct Entry {
    owner: SessionToken,
    playlist: String,
    snapshot: Snapshot,
    rows: HashMap<Uuid, ObservedRow>,
    retired: Instant,
}

#[derive(Default)]
pub(super) struct Archive(VecDeque<Entry>);

impl Archive {
    pub(super) fn retain_owner(&mut self, owner: &SessionToken) {
        self.0.retain(|entry| {
            entry.owner == *owner
                && entry.retired.elapsed() < LIFETIME
                && entry.snapshot.api.strong_count() > 0
        });
    }

    pub(super) fn remember(&mut self, visit: Visit) {
        let Some(snapshot) = visit.snapshot else {
            return;
        };
        if visit.rows.is_empty() || snapshot.api.strong_count() == 0 {
            return;
        }
        self.0.push_back(Entry {
            owner: visit.owner,
            playlist: visit.playlist,
            snapshot,
            rows: visit.rows,
            retired: Instant::now(),
        });
        let mut rows = self.0.iter().map(|entry| entry.rows.len()).sum::<usize>();
        while self.0.len() > MAX_VISITS || rows > MAX_ARCHIVED_ROWS {
            let Some(oldest) = self.0.pop_front() else {
                break;
            };
            rows -= oldest.rows.len();
        }
    }

    pub(super) fn row(&self, current: &Visit, receipt: &Uuid) -> Option<&ObservedRow> {
        let snapshot = current.snapshot.as_ref()?;
        self.0.iter().rev().find_map(|entry| {
            if entry.owner != current.owner
                || entry.playlist != current.playlist
                || entry.retired.elapsed() >= LIFETIME
                || !entry.snapshot.same_source(snapshot)
            {
                return None;
            }
            let row = entry.rows.get(receipt)?;
            // A fresh observation at this ordinal is authoritative, including
            // a freshly hidden row. Only a Back-retained suffix may use this.
            matches!(
                current.ordinal.get(row.ordinal),
                None | Some(Ordinal::Unobserved)
            )
            .then_some(row)
        })
    }
}
