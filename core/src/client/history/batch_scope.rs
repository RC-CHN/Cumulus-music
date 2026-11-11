use super::HistorySelectionScope;
use crate::{
    Error, Result,
    history::{HistoryKind, HistoryRecord, HistoryService, HistorySnapshot},
    storage::StateTransaction,
};
use std::collections::BTreeSet;

impl HistorySelectionScope {
    pub(in crate::client) fn ordered_rows<'a>(
        &self,
        snapshot: &'a HistorySnapshot,
        maximum: usize,
    ) -> Result<Vec<&'a HistoryRecord>> {
        if snapshot.revision != self.revision {
            return Err(Error::QueueChanged);
        }
        if !(1..=maximum).contains(&self.ids.len()) {
            return Err(Error::InvalidInput("invalid history batch selection size"));
        }
        let wanted: BTreeSet<_> = self.ids.iter().map(String::as_str).collect();
        let rows: Vec<_> = snapshot
            .entries
            .iter()
            .filter(|row| wanted.contains(row.id.as_str()))
            .collect();
        if !rows.iter().map(|row| &row.id).eq(self.ids.iter()) {
            return Err(Error::QueueChanged);
        }
        Ok(rows)
    }

    pub(in crate::client) fn verify_in(
        &self,
        tx: &StateTransaction<'_>,
        owner: &str,
        kind: HistoryKind,
    ) -> Result<()> {
        if HistoryService::snapshot_in(tx, owner, kind)?.revision != self.revision {
            return Err(Error::QueueChanged);
        }
        Ok(())
    }
}
