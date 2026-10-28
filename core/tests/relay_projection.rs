use cumulus_core::{
    playback::{PlaybackMode, QueueEntry, QueueSnapshot},
    relay::{
        RelayCommand, RelaySource, RelaySourceKind,
        publisher::{
            PublicationEvent, PublicationFrame,
            player::{events, project},
        },
    },
};
use serde_json::{Value, json};
fn queue(ids: &[u64]) -> QueueSnapshot {
    let items: Vec<_> = ids
        .iter()
        .map(|id| QueueEntry {
            entry_id: format!("e{id}"),
            resource: json!({"id":id,"level":"lossless","url":"PRIVATE","cookie":"PRIVATE"}),
            source: json!({"kind":"playlist","id":77}),
        })
        .collect();
    QueueSnapshot {
        order: items.iter().map(|i| i.entry_id.clone()).collect(),
        current: items.first().map(|i| i.entry_id.clone()),
        items,
        mode: PlaybackMode::OrderLoop,
    }
}
fn frame(ids: &[u64]) -> PublicationFrame {
    project(&queue(ids), None, false).unwrap()
}
fn apply(before: &[u64], events: Vec<PublicationEvent>) -> Vec<u64> {
    let mut values = before.to_vec();
    for event in events {
        let PublicationEvent::Command { command } = event else {
            continue;
        };
        match command {
            RelayCommand::Delete { resources } => {
                values.retain(|id| !resources.iter().any(|r| r.id == id.to_string()))
            }
            RelayCommand::Clear => values.clear(),
            RelayCommand::Insert { resources, pre_id }
            | RelayCommand::Move { resources, pre_id } => {
                let ids: Vec<u64> = resources.iter().map(|r| r.id.parse().unwrap()).collect();
                values.retain(|id| !ids.contains(id));
                let position = pre_id
                    .map(|id| {
                        values
                            .iter()
                            .position(|n| n.to_string() == id)
                            .expect("predecessor exists")
                            + 1
                    })
                    .unwrap_or(0);
                values.splice(position..position, ids);
            }
        }
    }
    values
}
fn permutations(values: &mut [u64], start: usize, check: &mut impl FnMut(&[u64])) {
    if start == values.len() {
        check(values);
        return;
    }
    for i in start..values.len() {
        values.swap(start, i);
        permutations(values, start + 1, check);
        values.swap(start, i);
    }
}
#[test]
fn every_small_permutation_and_mixed_edit_reconstructs_the_actual_queue() {
    let before = frame(&[1, 2, 3, 4, 5, 6]);
    let mut count = 0;
    permutations(&mut [1, 2, 3, 4, 5, 6], 0, &mut |ids| {
        assert_eq!(
            apply(
                &[1, 2, 3, 4, 5, 6],
                events(&before, &frame(ids), false, false)
            ),
            ids
        );
        count += 1;
        let mut mixed = ids
            .iter()
            .copied()
            .filter(|n| n % 2 == 0)
            .collect::<Vec<_>>();
        mixed.insert(0, 91);
        mixed.insert(2, 92);
        mixed.push(93);
        assert_eq!(
            apply(
                &[1, 2, 3, 4, 5, 6],
                events(&before, &frame(&mixed), false, false)
            ),
            mixed
        );
    });
    assert_eq!(count, 720);
}
#[test]
fn append_uses_actual_tail_and_bulk_delete_keeps_voice_types() {
    let before = frame(&[1, 2]);
    let after = frame(&[1, 2, 3, 4]);
    let changes = events(&before, &after, false, false);
    assert!(
        matches!(&changes[0],PublicationEvent::Command{command:RelayCommand::Insert{pre_id:Some(id),resources}} if id=="2" && resources.len()==2)
    );
    let mut before = queue(&[1, 2, 3]);
    before.items[1].resource["voice_id"] = json!(9007199254740993123u64);
    let after = queue(&[3]);
    let changes = events(
        &project(&before, None, false).unwrap(),
        &project(&after, None, false).unwrap(),
        false,
        false,
    );
    let PublicationEvent::Command {
        command: RelayCommand::Delete { resources },
    } = &changes[0]
    else {
        panic!("bulk deletion")
    };
    assert_eq!(
        serde_json::to_value(resources).unwrap(),
        json!([{"id":"1","type":"song"},{"id":"9007199254740993123","type":"voice"}])
    );
}
#[test]
fn source_projection_preserves_collection_and_never_exports_private_media_data() {
    let before = project(&queue(&[1, 2]), None, false).unwrap();
    let mut after = queue(&[2, 1]);
    after.items[0].source = json!({"kind":"album","id":99});
    let projected = project(&after, Some(&before), false).unwrap();
    assert_eq!(
        projected.source,
        Some(RelaySource::Collection {
            source_type: RelaySourceKind::Playlist,
            id: "77".into()
        })
    );
    assert!(
        !serde_json::to_string(&projected)
            .unwrap()
            .contains("PRIVATE")
    );
    after.items[0].source =
        json!({"kind":"podcast","scope":{"kind":"radio","id":"15","ascending":false}});
    assert_eq!(
        project(&after, None, true).unwrap().source,
        Some(RelaySource::Collection {
            source_type: RelaySourceKind::Podcast,
            id: "15".into()
        })
    );
    for kind in ["local", "cloud", "downloads", "download", "fm", "scene_fm"] {
        after.items[0].source = json!({"kind":kind});
        assert!(!project(&after, None, true).unwrap().can_publish());
    }
    after.items[0].source = json!({"kind":"songs"});
    after.items[1].resource["path"] = json!("PRIVATE-path");
    assert!(project(&after, None, true).unwrap().items.is_empty());
}
#[test]
fn randomized_traversal_does_not_reorder_wire_queue_and_handoff_suppresses_only_that_mode_event() {
    let mut q = queue(&[1, 2, 3]);
    let before = project(&q, None, false).unwrap();
    q.mode = PlaybackMode::Random;
    q.order.reverse();
    let after = project(&q, Some(&before), false).unwrap();
    assert!(events(&before, &after, false, true).is_empty());
    let events = events(&before, &after, false, false);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        PublicationEvent::Mode {
            from_handoff: false
        }
    ));
    assert_eq!(
        after
            .items
            .iter()
            .map(|i| i.resource.id.as_str())
            .collect::<Vec<_>>(),
        vec!["1", "2", "3"]
    );
}
#[test]
fn duplicate_resource_ids_and_oversized_reorders_use_explicit_snapshots() {
    let mut q = queue(&[1, 2, 3]);
    q.items[1].resource["id"] = json!(1);
    let before = project(&q, None, false).unwrap();
    q.items.swap(0, 2);
    let after = project(&q, Some(&before), false).unwrap();
    assert!(matches!(
        events(&before, &after, false, false).as_slice(),
        [PublicationEvent::Snapshot]
    ));
    let ids: Vec<_> = (1..=2000).collect();
    let mut reverse = ids.clone();
    reverse.reverse();
    assert!(matches!(
        events(&frame(&ids), &frame(&reverse), false, false).as_slice(),
        [PublicationEvent::Snapshot]
    ));
    let empty = project(&QueueSnapshot::default(), None, false).unwrap();
    assert!(matches!(
        events(&before, &empty, false, false).as_slice(),
        [PublicationEvent::Clear]
    ));
    let raw: Value = serde_json::to_value(empty).unwrap();
    assert!(raw["source"].is_null());
}
