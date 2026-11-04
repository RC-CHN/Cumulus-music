use super::*;
use crate::playback::queue_more::{
    QueueMoreView, ROOM_BINDING, active, anchor, clear_active, seed, separator,
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
            let after = next
                .current
                .as_ref()
                .and_then(|current| next.order.iter().position(|id| id == current))
                .map_or(next.order.len(), |index| index + 1);
            let mut ids = ids.into_iter();
            if let Some(first) = ids.next() {
                // M/1384 setRandomNextInPlayingList singles out the first
                // added recommendation; the others remain in the new tail.
                next.order.insert(after, first);
                next.order.extend(ids);
            }
        } else {
            next.order = next
                .items
                .iter()
                .map(|entry| entry.entry_id.clone())
                .collect();
        }
        debug_assert!(next.items.iter().any(|entry| active(&next, entry)));
        anchor::reconcile(&self.state, &mut next, false);
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
            snapshot.items.iter().map(seed).collect::<Vec<_>>(),
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

    #[test]
    fn random_queue_places_only_the_first_recommendation_next() {
        let temporary = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(temporary.path()).unwrap());
        let mut queue = PlaybackQueue::new(Some(store), "123").unwrap();
        queue
            .replace(
                vec![track(1), track(2), track(3)],
                0,
                json!({"kind":"daily_recommendations"}),
            )
            .unwrap();
        queue.set_mode(PlaybackMode::Random).unwrap();
        let before = queue.snapshot();
        queue.select(&before.order[0]).unwrap();
        queue.insert_more(vec![song("4"), song("5")], 3).unwrap();
        let after = queue.snapshot();
        let inserted = &after.items[3..];
        assert_eq!(
            after.order,
            vec![
                before.order[0].clone(),
                inserted[0].entry_id.clone(),
                before.order[1].clone(),
                before.order[2].clone(),
                inserted[1].entry_id.clone(),
            ]
        );
    }

    fn anchor_fixture() -> (
        tempfile::TempDir,
        Arc<StateStore>,
        PlaybackQueue,
        [String; 4],
    ) {
        let temporary = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(temporary.path()).unwrap());
        let mut queue = PlaybackQueue::new(Some(store.clone()), "123").unwrap();
        queue
            .replace(
                vec![track(1), track(2)],
                0,
                json!({"kind":"daily_recommendations"}),
            )
            .unwrap();
        queue.insert_more(vec![song("51"), song("52")], 1).unwrap();
        let ids = queue
            .snapshot()
            .items
            .into_iter()
            .map(|row| row.entry_id)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        (temporary, store, queue, ids)
    }

    fn move_rows(queue: &mut PlaybackQueue, ids: &[&str], offset: usize) {
        queue
            .reorder_displayed(&super::super::super::QueueMovement {
                entry_ids: ids.iter().map(|id| (*id).to_owned()).collect(),
                offset,
            })
            .unwrap();
    }

    fn active_ids(queue: &PlaybackQueue) -> Vec<&str> {
        queue
            .state
            .items
            .iter()
            .filter(|row| active(&queue.state, row))
            .map(|row| row.entry_id.as_str())
            .collect()
    }

    #[test]
    fn moving_the_anchor_down_uses_old_successors_in_new_suffix_and_cold_reseeds() {
        let (_temporary, store, mut queue, [a, r1, r2, b]) = anchor_fixture();
        let mut observer = PlaybackQueue::new(Some(store.clone()), "123").unwrap();
        assert_eq!(anchor::index_and_id(&queue.state), Some((1, r1.as_str())));
        move_rows(&mut queue, &[&r1, &r2], 4);
        assert_eq!(
            queue
                .state
                .items
                .iter()
                .map(|row| row.entry_id.as_str())
                .collect::<Vec<_>>(),
            [&a, &b, &r1, &r2]
        );
        assert_eq!(active_ids(&queue), [&r1, &r2]);
        assert_eq!(anchor::index_and_id(&queue.state), Some((3, r2.as_str())));
        let raw = store.get("123", "playback", "queue").unwrap().unwrap();
        assert_eq!(raw["more"]["anchor"], json!({"entry":r2}));
        assert!(observer.reload().unwrap());
        assert_eq!(
            anchor::index_and_id(&observer.state),
            Some((3, r2.as_str()))
        );
        let cold = PlaybackQueue::new(Some(store), "123").unwrap();
        assert_eq!(anchor::index_and_id(&cold.state), Some((2, r1.as_str())));
        assert_eq!(active_ids(&cold), [&r1, &r2]);

        // Same displayed order, but the real boundary is now index 3 rather
        // than the first active song at index 2.
        let revision = queue.revision();
        move_rows(&mut queue, &[&r1], 3);
        assert_ne!(queue.revision(), revision);
        assert_eq!(active_ids(&queue), [&r2]);
        assert_eq!(anchor::index_and_id(&queue.state), Some((3, r2.as_str())));
        assert_eq!(queue.selected().unwrap().entry_id, a);
    }

    #[test]
    fn deleting_anchor_can_transfer_to_ordinary_row_and_reset_clears_index_zero() {
        let (_temporary, store, mut queue, [a, r1, r2, b]) = anchor_fixture();
        queue
            .reorder(&[a.clone(), r1.clone(), b.clone(), r2.clone()])
            .unwrap();
        queue.remove(&[r1]).unwrap();
        assert_eq!(active_ids(&queue), [&r2]);
        assert_eq!(anchor::index_and_id(&queue.state), Some((1, b.as_str())));
        queue.reorder(&[b.clone(), a, r2.clone()]).unwrap();
        assert_eq!(anchor::index_and_id(&queue.state), Some((0, b.as_str())));
        assert_eq!(
            queue.more_view(true).unwrap().separator_before,
            Some(b.clone())
        );

        move_rows(&mut queue, &[&r2], 0);
        assert_eq!(anchor::index_and_id(&queue.state), Some((1, b.as_str())));
        assert!(active_ids(&queue).is_empty());

        queue.insert_more(vec![song("53"), song("54")], 2).unwrap();
        let current = queue.state.items[2].entry_id.clone();
        queue.select(&current).unwrap();
        queue.set_more_preference(false).unwrap();
        assert!(active_ids(&queue).is_empty());
        assert!(anchor::index_and_id(&queue.state).is_none());
        assert!(queue.more_view(false).unwrap().separator_before.is_none());
        assert_eq!(queue.state.current.as_deref(), Some(current.as_str()));
        assert_eq!(queue.len(), 4); // keep the demoted row and active current
        let raw = store.get("123", "playback", "queue").unwrap().unwrap();
        assert_eq!(raw["more"]["anchor"], "none");
        assert!(
            PlaybackQueue::new(Some(store), "123")
                .unwrap()
                .more_view(false)
                .unwrap()
                .separator_before
                .is_none()
        );
    }

    #[test]
    fn a_persisted_no_divider_differs_from_missing_legacy_metadata() {
        let (_temporary, store, mut queue, [_, r1, r2, _]) = anchor_fixture();
        move_rows(&mut queue, &[&r1, &r2], 4);
        let mut observer = PlaybackQueue::new(Some(store.clone()), "123").unwrap();
        move_rows(&mut queue, &[&r2], 4);
        assert!(anchor::index_and_id(&queue.state).is_none());
        assert_eq!(active_ids(&queue), [&r1, &r2]);
        assert!(observer.reload().unwrap());
        assert!(anchor::index_and_id(&observer.state).is_none());

        let mut old = store.get("123", "playback", "queue").unwrap().unwrap();
        assert_eq!(old["more"]["anchor"], "none");
        old["more"].as_object_mut().unwrap().remove("anchor");
        let legacy = QueueSnapshot::load(Some(&old)).unwrap();
        assert_eq!(anchor::index_and_id(&legacy), Some((2, r1.as_str())));
        store.put("123", "playback", "queue", &old).unwrap();
        assert!(observer.reload().unwrap());
        assert_eq!(
            anchor::index_and_id(&observer.state),
            Some((2, r1.as_str()))
        );

        // A new actual list update following a no-anchor result re-arms the
        // first active recommendation; the no-anchor commit itself did not.
        let (_temporary, _, mut queue, [a, r1, r2, _]) = anchor_fixture();
        move_rows(&mut queue, &[&r1, &r2], 4);
        move_rows(&mut queue, &[&r2], 4);
        move_rows(&mut queue, &[&a], 0);
        assert_eq!(anchor::index_and_id(&queue.state), Some((2, r1.as_str())));
    }
}
