use super::*;
use crate::playback::queue_more::{
    QueueMoreView, ROOM_BINDING, active, clear_active, seed, separator,
};

impl PlaybackQueue {
    pub(crate) fn more_unattached(&self) -> Result<bool> {
        let Some(store) = &self.store else {
            return Ok(false);
        };
        Ok(store.get(&self.owner, ROOM_BINDING, "current")?.is_none())
    }

    pub(crate) fn more_snapshot(&self) -> Option<&super::super::queue_more::QueueMoreOrigin> {
        self.state.more.as_ref()
    }

    pub(crate) fn more_view(&self, enabled: bool) -> Option<QueueMoreView> {
        let origin = self.state.more.as_ref()?;
        let visible =
            !self.is_fm() && self.state.intelligence.is_none() && self.more_unattached().ok()?;
        visible.then(|| QueueMoreView {
            visible,
            enabled,
            can_toggle: true,
            separator_before: enabled.then(|| separator(&self.state)).flatten(),
            prefetch_on_open: enabled && origin.prefetch_on_open && origin.active.is_empty(),
        })
    }

    pub(crate) fn set_more_preference(&mut self, enabled: bool) -> Result<()> {
        let mut next = self.snapshot();
        let origin = next
            .more
            .as_mut()
            .ok_or(Error::State("queue does not support recommendations"))?;
        if !enabled {
            origin.prefetch_on_open = false;
            clear_active(&mut next);
        }
        self.commit_more(next, Some(enabled))
    }

    /// M/1384 uses the seed count, not the physical tail, as its display offset.
    /// Keep the existing selection and original random ordering of the existing queue.
    pub(crate) fn insert_more(&mut self, rows: Vec<QueueEntry>, seed_count: usize) -> Result<()> {
        let mut next = self.snapshot();
        let Some(origin) = &mut next.more else {
            return Err(Error::QueueChanged);
        };
        let mut existing: HashSet<_> = next.items.iter().filter_map(seed).collect();
        let capacity = 10_000usize.saturating_sub(next.items.len());
        let rows: Vec<_> = rows
            .into_iter()
            .filter(|row| seed(row).is_some_and(|id| existing.insert(id)))
            .take(capacity)
            .collect();
        if rows.is_empty() {
            return Ok(());
        }
        let index = seed_count.min(next.items.len());
        origin
            .active
            .extend(rows.iter().map(|entry| entry.entry_id.clone()));
        let ids = rows
            .iter()
            .map(|entry| entry.entry_id.clone())
            .collect::<Vec<_>>();
        next.items.splice(index..index, rows);
        if next.mode == PlaybackMode::Random {
            next.order.extend(ids);
        } else {
            next.order = next
                .items
                .iter()
                .map(|entry| entry.entry_id.clone())
                .collect();
        }
        debug_assert!(next.items.iter().any(|entry| active(&next, entry)));
        self.commit_more(next, None)
    }
}
