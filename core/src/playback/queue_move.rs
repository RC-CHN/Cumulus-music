//! An explicit move in the original displayed queue, independent of playback order.
use super::{QueueSnapshot, queue_more};
use crate::{Error, Result};
use serde::Deserialize;
use std::collections::HashSet;

/// A selected block and its insertion boundary in the *old* displayed order.
/// Native-only rows have no displayed index and cannot be moved by this input.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueMovement {
    pub entry_ids: Vec<String>,
    pub offset: usize,
}

impl QueueMovement {
    pub(crate) fn visible_order<'a>(&self, queue: &'a QueueSnapshot) -> Result<Vec<&'a str>> {
        let visible: Vec<_> = queue
            .items
            .iter()
            .filter(|item| item.visible_in_queue())
            .map(|item| item.entry_id.as_str())
            .collect();
        let selected: HashSet<_> = self.entry_ids.iter().map(String::as_str).collect();
        if self.offset > visible.len()
            || selected.is_empty()
            || selected.len() != self.entry_ids.len()
            || selected.len() > visible.len()
        {
            return Err(Error::InvalidInput("invalid displayed queue movement"));
        }
        let (mut before, mut moving, mut after) = (Vec::new(), Vec::new(), Vec::new());
        for (index, id) in visible.into_iter().enumerate() {
            if selected.contains(id) {
                moving.push(id);
            } else if index < self.offset {
                before.push(id);
            } else {
                after.push(id);
            }
        }
        if moving.len() != selected.len() {
            return Err(Error::InvalidInput(
                "moved entry is outside the displayed queue",
            ));
        }
        before.extend(moving);
        before.extend(after);
        Ok(before)
    }

    fn before_or_on_divider(&self, queue: &QueueSnapshot) -> bool {
        // M/1384 tests the pre-move index and the original insertion offset.
        // At index 0 the official model still has a boundary even though the
        // UI does not render a separator row.
        queue
            .items
            .iter()
            .filter(|entry| entry.visible_in_queue())
            .position(|entry| queue_more::active(queue, entry))
            .is_some_and(|index| self.offset <= index)
    }

    pub(crate) fn can_demote(&self, queue: &QueueSnapshot) -> bool {
        self.before_or_on_divider(queue)
            && queue
                .more
                .as_ref()
                .is_some_and(|more| self.entry_ids.iter().any(|id| more.active.contains(id)))
    }

    pub(super) fn demote(&self, previous: &QueueSnapshot, next: &mut QueueSnapshot) -> bool {
        if !self.can_demote(previous) {
            return false;
        }
        let Some(more) = &mut next.more else {
            return false;
        };
        let before = more.active.len();
        for id in &self.entry_ids {
            more.active.remove(id);
        }
        before != more.active.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::{QueueEntry, queue_more::QueueMoreOrigin};
    use serde_json::{Value, json};

    fn sample() -> QueueSnapshot {
        let items = [
            ("a", Value::Null),
            ("private", json!({"intelligence_transient":true})),
            ("b", Value::Null),
            ("r1", json!({"kind":"queue_more_recommendation"})),
            ("r2", json!({"kind":"queue_more_recommendation"})),
            ("r3", json!({"kind":"queue_more_recommendation"})),
        ]
        .into_iter()
        .map(|(id, source)| QueueEntry {
            entry_id: id.into(),
            resource: Value::Null,
            source,
        })
        .collect();
        let mut more = QueueMoreOrigin::new(&json!({"kind":"daily_recommendations"})).unwrap();
        more.active.extend(["r1".into(), "r2".into(), "r3".into()]);
        QueueSnapshot {
            items,
            more: Some(more),
            ..Default::default()
        }
    }

    fn movement(ids: &[&str], offset: usize) -> QueueMovement {
        QueueMovement {
            entry_ids: ids.iter().map(|id| (*id).to_owned()).collect(),
            offset,
        }
    }

    #[test]
    fn displaced_recommendations_stay_active_and_only_selected_rows_demote() {
        let old = sample();
        let mut next = old.clone();
        let normal = movement(&["b"], 5);
        assert_eq!(
            normal.visible_order(&old).unwrap(),
            ["a", "r1", "r2", "r3", "b"]
        );
        assert!(!normal.demote(&old, &mut next));
        assert_eq!(old.more, next.more);
        let group = movement(&["r3", "r1"], 2);
        assert_eq!(
            group.visible_order(&old).unwrap(),
            ["a", "b", "r1", "r3", "r2"]
        );
        assert!(group.demote(&old, &mut next));
        assert_eq!(
            next.more.unwrap().active.into_iter().collect::<Vec<_>>(),
            ["r2"]
        );
    }

    #[test]
    fn dropping_at_the_divider_can_change_only_the_marker() {
        let old = sample();
        let mut next = old.clone();
        let on_divider = movement(&["r1"], 2);
        assert_eq!(
            on_divider.visible_order(&old).unwrap(),
            ["a", "b", "r1", "r2", "r3"]
        );
        assert!(on_divider.demote(&old, &mut next));
        assert!(!next.more.as_ref().unwrap().active.contains("r1"));
        assert!(!movement(&["r2"], 3).demote(&old, &mut next));
        assert!(next.more.unwrap().active.contains("r2"));
    }

    #[test]
    fn cannot_move_a_hidden_unknown_or_duplicate_identity() {
        let old = sample();
        for invalid in [
            movement(&["private"], 0),
            movement(&["missing"], 0),
            movement(&["r1", "r1"], 0),
            movement(&["r1"], 6),
            movement(&[], 0),
        ] {
            assert!(invalid.visible_order(&old).is_err());
        }
        assert!(
            serde_json::from_value::<QueueMovement>(json!({
                "entry_ids":["r1"],"offset":0,"separator":1
            }))
            .is_err()
        );
    }
}
