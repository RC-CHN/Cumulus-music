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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::queue_more::QueueMoreSong;
    use crate::playback::queue_more::{KIND, PREFERENCE, recommended};
    use serde_json::json;

    fn track(id: u64) -> Value {
        json!({"id":id,"song":{"id":id,"name":"synthetic"}})
    }

    fn song(id: &str) -> QueueEntry {
        recommended(
            QueueMoreSong {
                id: id.into(),
                name: "synthetic recommended".into(),
                duration_ms: 8000,
                fee: None,
                artists: Vec::new(),
                album_id: None,
                album_name: String::new(),
                cover_url: None,
                algorithm: Some("never public".into()),
                privilege: None,
            },
            "standard",
        )
    }

    #[test]
    fn mixed_queue_uses_seed_offset_keeps_current_and_never_duplicate_rec_tracks() {
        let temporary = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(temporary.path()).unwrap());
        let mut queue = PlaybackQueue::new(Some(store.clone()), "123").unwrap();
        queue
            .replace(
                vec![track(1), json!({"voice_id":"7"}), track(2)],
                0,
                json!({"kind":"daily_recommendations"}),
            )
            .unwrap();
        let current = queue.selected().unwrap();
        queue
            .insert_more(vec![song("2"), song("3"), song("3"), song("4")], 2)
            .unwrap();
        let snapshot = queue.snapshot();
        assert_eq!(
            snapshot
                .items
                .iter()
                .map(|item| seed(item))
                .collect::<Vec<_>>(),
            vec![
                Some("1".into()),
                None,
                Some("3".into()),
                Some("4".into()),
                Some("2".into())
            ]
        );
        assert_eq!(queue.selected(), Some(current));
        assert_eq!(
            queue.more_view(true).unwrap().separator_before.as_ref(),
            Some(&snapshot.items[2].entry_id)
        );
        assert!(
            !serde_json::to_string(&snapshot)
                .unwrap()
                .contains("never public")
        );

        let recommended_id = snapshot.items[2].entry_id.clone();
        let selected = queue.select(&recommended_id).unwrap();
        queue.set_more_preference(false).unwrap();
        assert_eq!(queue.selected(), Some(selected.clone()));
        assert!(queue.more_view(false).unwrap().separator_before.is_none());
        assert!(
            queue
                .snapshot()
                .items
                .iter()
                .all(|row| !active(&queue.state, row))
        );
        assert_eq!(queue.len(), 4);

        let reopened = PlaybackQueue::new(Some(store.clone()), "123").unwrap();
        assert_eq!(reopened.selected(), Some(selected));
        assert_eq!(
            store.get("123", KIND, PREFERENCE).unwrap(),
            Some(json!(false))
        );
        assert_eq!(store.get("456", KIND, PREFERENCE).unwrap(), None);
        assert!(
            PlaybackQueue::new(Some(store), "456")
                .unwrap()
                .more_view(true)
                .is_none()
        );
    }

    #[test]
    fn room_gate_checks_in_the_same_commit_as_preferences_and_recommendations() {
        let temporary = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(temporary.path()).unwrap());
        let mut queue = PlaybackQueue::new(Some(store.clone()), "123").unwrap();
        queue
            .replace(
                vec![track(1), track(2)],
                0,
                json!({"kind":"style_recommendations"}),
            )
            .unwrap();
        let before = queue.snapshot();
        store
            .put("123", ROOM_BINDING, "current", &json!({"synthetic":true}))
            .unwrap();
        assert!(queue.more_view(true).is_none());
        assert!(queue.set_more_preference(false).is_err());
        assert!(queue.insert_more(vec![song("3")], 2).is_err());
        assert_eq!(queue.snapshot(), before);
        assert_eq!(store.get("123", KIND, PREFERENCE).unwrap(), None);
    }
}
