//! M3102 and M3085 dispatch a full-list replacement without a song playId.
use super::{
    Error, HistoryKind, PlaybackMode, QueueSnapshot, Result, ShuffleAnchor, Value, history_key,
    retain_admitted,
};
use rand::{Rng, seq::IndexedRandom};

pub(super) fn fm_return_mode(backup: Option<&Value>) -> Result<PlaybackMode> {
    let Some(raw) = backup else {
        // Older or incomplete FM state may have no ordinary backup. Its
        // SingleLoop/Sequential repeat is not an ordinary preference.
        return Ok(PlaybackMode::default());
    };
    let queue = QueueSnapshot::load(Some(raw))?;
    if queue.fm_source.is_some()
        || queue.intelligence.is_some()
        || queue.items.iter().any(|item| item.source["kind"] == "fm")
    {
        return Err(Error::State("invalid FM return queue"));
    }
    for item in &queue.items {
        crate::playback::PlaybackConstraint::from_resource(&item.resource)?;
    }
    Ok(queue.mode)
}

pub(super) fn plan(
    kind: HistoryKind,
    mode: PlaybackMode,
    mut resources: Vec<Value>,
    source: Value,
    anchor: Option<&ShuffleAnchor>,
    rng: &mut impl Rng,
) -> Result<QueueSnapshot> {
    use super::super::{local_batch, queue_playlist::insert_collection_by_key};
    // M3085:483–489 uses the ORIGINAL resource's top-level categoryId,
    // not a later admitted voice or its nested radio category.
    let mode = if kind == HistoryKind::Voice
        && !resources.first().is_some_and(|row| {
            matches!(
                row["voice"]["categoryId"].as_i64(),
                Some(3001 | 10002 | 2001)
            )
        }) {
        PlaybackMode::Sequential
    } else {
        mode
    };
    retain_admitted(&mut resources);
    if resources.is_empty() {
        return Err(Error::State("history selection has no playable resources"));
    }
    let mut queue = insert_collection_by_key(
        QueueSnapshot {
            mode,
            ..QueueSnapshot::default()
        },
        resources,
        source,
        false,
        true,
        history_key,
    );
    let first = &queue.items.first().ok_or(Error::QueueChanged)?.entry_id;
    // An explicit voice bookmark must still resolve to its exact programme.
    // If it cannot, keep the old queue instead of installing a list that
    // cannot select the requested programme.
    queue.current = Some(
        if let Some(anchor) = anchor {
            &queue
                .items
                .iter()
                .find(|row| anchor.matches(&row.resource))
                .ok_or(Error::QueueChanged)?
                .entry_id
        } else if mode == PlaybackMode::Random {
            &queue.items.choose(rng).ok_or(Error::QueueChanged)?.entry_id
        } else {
            first
        }
        .clone(),
    );
    if mode == PlaybackMode::Random {
        let inserted = queue
            .items
            .iter()
            .map(|row| row.entry_id.as_str())
            .collect();
        queue.order = local_batch::random_order(&queue, &inserted, first, false, rng);
    }
    Ok(queue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};
    use serde_json::json;

    #[test]
    fn history_play_all_has_no_implicit_head_in_random_mode() {
        let resources = vec![
            json!({"id":"9","privilege":{"pl":0}}),
            json!({"id":"8","privilege":{"pl":320000}}),
            json!({"id":"7","privilege":{"pl":320000}}),
        ];
        let mut start_ids = std::collections::BTreeSet::new();
        for seed in 0..16 {
            let queue = plan(
                HistoryKind::Song,
                PlaybackMode::Random,
                resources.clone(),
                json!({"kind":"history"}),
                None,
                &mut StdRng::seed_from_u64(seed),
            )
            .unwrap();
            assert!(
                queue
                    .items
                    .iter()
                    .map(|row| row.resource["id"].as_str())
                    .eq([Some("8"), Some("7"),])
            );
            start_ids.insert(
                queue.selected().unwrap().resource["id"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
            queue.validate().unwrap();
        }
        assert!(start_ids.iter().map(String::as_str).eq(["7", "8"]));
    }

    #[test]
    fn history_play_all_voice_mode_comes_from_the_original_top_level_category() {
        for (category, expected) in [
            (None, PlaybackMode::Sequential),
            (Some(10001), PlaybackMode::Sequential),
            (Some(3001), PlaybackMode::Random),
            (Some(10002), PlaybackMode::Random),
            (Some(2001), PlaybackMode::Random),
        ] {
            let resource = json!({"id":"9001","voice_id":"1","voice":{
                "id":"1","categoryId":category,"radio":{"categoryId":3001}
            }});
            let queue = plan(
                HistoryKind::Voice,
                PlaybackMode::Random,
                vec![resource],
                json!({"kind":"history"}),
                None,
                &mut StdRng::seed_from_u64(0),
            )
            .unwrap();
            assert_eq!(queue.mode, expected);
        }
    }
}
