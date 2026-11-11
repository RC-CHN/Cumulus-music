use super::{OrdinarySkipDecision as Decision, OrdinarySkipTrigger as Trigger, *};
use crate::playback::preflight::OrdinaryCandidateRow;
use serde_json::json;

fn row(id: usize, playable: bool) -> QueueEntry {
    let privilege = if playable {
        json!({"st":0,"pl":128000,"flag":0,"fee":0})
    } else {
        json!({"st":-200,"pl":0,"flag":0,"fee":0})
    };
    QueueEntry {
        entry_id: format!("e{id}"),
        resource: json!({"id":id,"song":{"id":id,"privilege":privilege}}),
        source: json!({"kind":"playlist"}),
    }
}

fn queue(statuses: impl IntoIterator<Item = bool>) -> QueueSnapshot {
    let items: Vec<_> = statuses
        .into_iter()
        .enumerate()
        .map(|(i, online)| row(i + 1, online))
        .collect();
    let order: Vec<_> = items.iter().map(|row| row.entry_id.clone()).collect();
    QueueSnapshot {
        current: order.first().cloned(),
        mode: PlaybackMode::Sequential,
        items,
        order,
        ..Default::default()
    }
}

fn facts(q: &QueueSnapshot) -> OrdinaryQueuePreflight {
    let rows: Vec<_> = q
        .items
        .iter()
        .map(|entry| OrdinaryCandidateRow {
            entry_id: entry.entry_id.clone(),
            original_song_id: identifier(&entry.resource["id"]).unwrap(),
            candidate: if plain_copyright_denial(entry) {
                OrdinaryCandidate::Unavailable
            } else {
                OrdinaryCandidate::Online
            },
            no_download_publication: true,
            publication: None,
        })
        .collect();
    let candidate_order = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| (row.candidate == OrdinaryCandidate::Online).then_some(index))
        .collect();
    OrdinaryQueuePreflight {
        rows,
        candidate_order,
    }
}

#[test]
fn ordered_native_tail_looks_up_the_final_replacement_id_in_the_original_queue() {
    let q = queue([true, false, false, false]);
    let mut scan = facts(&q);
    scan.rows[3].candidate = OrdinaryCandidate::SameRecording { id: "500".into() };
    scan.candidate_order.push(3);
    assert_eq!(
        after_initial_denial(&q, &scan, Trigger::NativeEof).unwrap(),
        Decision::StopAtDeniedTail {
            attempted: "e2".into()
        },
    );
    // The substitute now matches an existing display row after the attempted
    // row, so the tail branch must not fire; transport follows those rows.
    scan.rows[3].candidate = OrdinaryCandidate::SameRecording { id: "3".into() };
    assert_eq!(
        after_initial_denial(&q, &scan, Trigger::NativeEof).unwrap(),
        Decision::Unchanged(SkipBoundary::AlternateOrLocalHandoff),
    );
}

#[test]
fn native_physical_tail_and_single_repeat_never_use_skip_and_explicit_sources_are_excluded() {
    let mut q = queue([true, false]);
    for trigger in [Trigger::DirectQueue, Trigger::ExplicitCatalog] {
        assert_eq!(
            after_initial_denial(&q, &facts(&q), trigger).unwrap(),
            Decision::Unchanged(SkipBoundary::ExcludedTrigger),
        );
    }
    q.current = Some("e2".into());
    assert_eq!(
        after_initial_denial(&q, &facts(&q), Trigger::NativeEof).unwrap(),
        Decision::Unchanged(SkipBoundary::NaturalEndOrRepeat),
    );
    q.current = Some("e1".into());
    q.mode = PlaybackMode::SingleLoop;
    assert_eq!(
        after_initial_denial(&q, &facts(&q), Trigger::NativeEof).unwrap(),
        Decision::Unchanged(SkipBoundary::NaturalEndOrRepeat),
    );
}

#[test]
fn manual_previous_wraps_and_random_uses_transport_order_not_candidate_order() {
    let mut q = queue([true, false, true, false]);
    assert_eq!(
        after_initial_denial(&q, &facts(&q), Trigger::MiniBar(Step::Previous)).unwrap(),
        Decision::PlayExisting {
            attempted: "e4".into(),
            target: "e3".into(),
            denial_count: 1,
            at_ordered_tail: true,
        },
    );
    q.mode = PlaybackMode::Random;
    q.order = ["e1", "e4", "e2", "e3"].map(String::from).to_vec();
    assert_eq!(
        after_initial_denial(&q, &facts(&q), Trigger::MiniBar(Step::Next)).unwrap(),
        Decision::PlayExisting {
            attempted: "e4".into(),
            target: "e3".into(),
            denial_count: 2,
            at_ordered_tail: false,
        },
    );
}

#[test]
fn empty_scan_pauses_but_a_playable_track_after_the_error_limit_is_not_selected() {
    let empty = queue([false, false, false]);
    assert_eq!(
        after_initial_denial(&empty, &facts(&empty), Trigger::MiniBar(Step::Next)).unwrap(),
        Decision::PauseCurrent {
            attempted: "e2".into()
        },
    );
    let cap = queue(
        [true]
            .into_iter()
            .chain(std::iter::repeat_n(false, 100))
            .chain([true]),
    );
    assert_eq!(
        after_initial_denial(&cap, &facts(&cap), Trigger::MiniBar(Step::Next)).unwrap(),
        Decision::StopAtErrorLimit {
            attempted: "e2".into(),
            denial_count: 100
        },
    );
}

#[test]
fn local_only_or_ambiguous_publications_are_not_played_or_skipped_as_plain_catalogue() {
    let q = queue([true, false, true]);
    let mut scan = facts(&q);
    scan.rows[2].candidate = OrdinaryCandidate::AccountDownload;
    scan.rows[2].no_download_publication = false;
    assert_eq!(
        after_initial_denial(&q, &scan, Trigger::MiniBar(Step::Next)).unwrap(),
        Decision::Unchanged(SkipBoundary::AlternateOrLocalHandoff),
    );
    scan.rows[1].no_download_publication = false;
    assert_eq!(
        after_initial_denial(&q, &scan, Trigger::MiniBar(Step::Next)).unwrap(),
        Decision::Unchanged(SkipBoundary::InitialRightsNotProven),
    );
}
