use cumulus_core::playback::{PlaybackMode, mode::ModeFamily};
use serde::Deserialize;

#[derive(Deserialize)]
struct Vector {
    family: ModeFamily,
    current: PlaybackMode,
    next: PlaybackMode,
}
#[derive(Deserialize)]
struct Frozen {
    vectors: Vec<Vector>,
}
#[test]
fn native_cycles_match_frozen_official_m7_vectors() {
    let fixture: Frozen =
        serde_json::from_str(include_str!("fixtures/official-playback-mode.json")).unwrap();
    assert_eq!(fixture.vectors.len(), 12);
    for row in fixture.vectors {
        assert_eq!(row.family.next(row.current), row.next);
        assert!(row.family.supports(row.next));
    }
}
#[test]
fn fm_mode_switch_is_separate_from_ordinary_and_shared_queue_cycles() {
    use PlaybackMode::*;
    assert_eq!(ModeFamily::Fm.choices(), &[Sequential, SingleLoop]);
    assert_eq!(ModeFamily::Fm.next(Sequential), SingleLoop);
    assert_eq!(ModeFamily::Fm.next(SingleLoop), Sequential);
    assert!(!ModeFamily::Fm.supports(Random));
    assert!(!ModeFamily::Fm.supports(OrderLoop));
    assert!(!ModeFamily::Together.supports(Sequential));
}
