use super::*;
use crate::playback::QueueEntry;
use rand::{SeedableRng, rngs::StdRng};
use serde_json::json;

fn song(id: &str, pl: u32) -> Value {
    json!({"id":id,"privilege":{"pl":pl}})
}

fn voice(id: &str, media: &str) -> Value {
    json!({"id":media,"voice_id":id})
}

#[test]
fn row_next_empty_queue_starts_clicked_song_or_voice_even_in_random() {
    for mode in [PlaybackMode::Sequential, PlaybackMode::Random] {
        for (first, clicked, field, require_admitted) in [
            (song("30", 320000), song("20", 320000), "id", true),
            (voice("30", "3001"), voice("20", "2001"), "voice_id", false),
        ] {
            let queue = plan(
                QueueSnapshot {
                    mode,
                    ..QueueSnapshot::default()
                },
                vec![first, clicked],
                json!({"kind":"history"}),
                true,
                BatchAnchor::RowMenu {
                    index: 1,
                    require_admitted,
                },
                &mut StdRng::seed_from_u64(5),
            )
            .unwrap();
            assert_eq!(queue.mode, mode);
            assert_eq!(queue.selected().unwrap().resource[field], "20");
            assert!(
                queue
                    .items
                    .iter()
                    .map(|entry| entry.resource[field].as_str())
                    .eq([Some("30"), Some("20")])
            );
            queue.validate().unwrap();
        }
    }
}

#[test]
fn row_next_queue_only_keeps_current_and_forces_first_admitted_random_successor() {
    let mut queue = QueueSnapshot {
        mode: PlaybackMode::Random,
        current: Some("current".into()),
        ..QueueSnapshot::default()
    };
    for (key, id) in [("current", "10"), ("tail", "11"), ("old-copy", "30")] {
        queue.items.push(QueueEntry {
            entry_id: key.into(),
            resource: song(id, 128000),
            source: json!({"kind":"incumbent"}),
        });
        queue.order.push(key.into());
    }
    let current = queue.selected();
    let queue = plan(
        queue,
        vec![song("99", 0), song("30", 320000), song("20", 320000)],
        json!({"kind":"history"}),
        false,
        BatchAnchor::RowMenu {
            index: 2,
            require_admitted: true,
        },
        &mut StdRng::seed_from_u64(5),
    )
    .unwrap();
    assert_eq!(queue.selected(), current);
    assert!(
        queue
            .items
            .iter()
            .map(|entry| entry.resource["id"].as_str())
            .eq([Some("10"), Some("30"), Some("20"), Some("11")])
    );
    assert_eq!(queue.items[1].entry_id, "old-copy");
    let current_index = queue.order.iter().position(|id| id == "current").unwrap();
    assert_eq!(queue.order[current_index + 1], "old-copy");
    queue.validate().unwrap();
}

#[test]
fn row_next_rejects_an_unplayable_clicked_song_instead_of_promoting_the_group() {
    let queue = QueueSnapshot::default();
    let result = plan(
        queue,
        vec![song("30", 320000), song("20", 0)],
        json!({"kind":"history"}),
        false,
        BatchAnchor::RowMenu {
            index: 1,
            require_admitted: true,
        },
        &mut StdRng::seed_from_u64(5),
    );
    assert!(matches!(
        result,
        Err(Error::State("clicked history resource cannot be played"))
    ));
}
