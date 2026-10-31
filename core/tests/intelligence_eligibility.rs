use cumulus_core::playback::intelligence::eligibility::online_candidate;
use serde_json::{Value, json};

#[test]
fn online_candidates_match_executed_official_m23_bodies() {
    let frozen: Value = serde_json::from_str(include_str!(
        "fixtures/official-intelligence-eligibility.json"
    ))
    .unwrap();
    let cases = frozen["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 83);
    for case in cases {
        assert_eq!(
            online_candidate(&case["song"]).unwrap(),
            case["canPlay"].as_bool().unwrap(),
            "{}",
            case["song"]
        );
    }
}

#[test]
fn malformed_privileges_are_explicit_errors_instead_of_fabricated_rights() {
    for song in [
        json!({"privilege":true}),
        json!({"privilege":{"maxPlayBr":"NaN"}}),
        json!({"privilege":{"maxPlayBr":0,"flag":{}}}),
    ] {
        assert!(online_candidate(&song).is_err());
    }
}
