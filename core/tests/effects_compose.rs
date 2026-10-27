use cumulus_core::effects::compose::{self, Composition};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Case {
    name: String,
    input: Value,
    result: Value,
}
#[derive(Deserialize)]
struct Corpus {
    version: String,
    cases: Vec<Case>,
}
fn corpus() -> Corpus {
    serde_json::from_str(include_str!("fixtures/effects-compose-official.json")).unwrap()
}
fn canonical(value: &Composition) -> Value {
    serde_json::from_str(
        &cumulus_core::protocol::wire::serialization::js_json(
            &serde_json::to_value(value).unwrap(),
        )
        .unwrap(),
    )
    .unwrap()
}
fn compare(case: &Case) {
    let before = case.input.clone();
    match compose::compose(&case.input) {
        Ok(result) => {
            assert_eq!(case.result["success"], true, "{}", case.name);
            let expected: Composition = serde_json::from_value(case.result.clone()).unwrap();
            // JS has one Number type; JSON 0 and 0.0 carry the same value.
            // Canonicalize numeric spelling only, with no epsilon tolerance.
            assert_eq!(canonical(&result), canonical(&expected), "{}", case.name);
        }
        Err(error) => assert_eq!(case.result["success"], false, "{}: {error}", case.name),
    }
    assert_eq!(case.input, before);
}
#[test]
fn typed_builtin_selections_match_official_fallback_and_do_not_share_mutation() {
    use compose::Enhancement::*;
    let all = [Bass, Treble, Vocal, Vibe, Live];
    let mut checked = 0;
    for case in corpus()
        .cases
        .iter()
        .filter(|case| case.name.starts_with("builtin-"))
    {
        let bits: usize = case.name.rsplit('-').next().unwrap().parse().unwrap();
        let selected: Vec<_> = all
            .iter()
            .enumerate()
            .filter_map(|(i, value)| ((bits >> i) & 1 == 1).then_some(*value))
            .collect();
        let result =
            compose::compose_builtin(case.input.get("songJson"), &selected, false).unwrap();
        let expected: Composition = serde_json::from_value(case.result.clone()).unwrap();
        assert_eq!(canonical(&result), canonical(&expected), "{}", case.name);
        checked += 1;
    }
    assert_eq!(checked, 64);
    let before = compose::builtin_base();
    let mut changed = compose::builtin_base();
    changed["key_transparent"]["effect"]["eq"]["eqs"][0] = json!(100);
    assert_eq!(compose::builtin_base(), before);
    assert!(serde_json::from_value::<compose::Enhancement>(json!("unknown")).is_err());
}
#[test]
fn official_composer_enhancement_rules_order_coercion_and_merges() {
    let corpus = corpus();
    assert_eq!(corpus.version, compose::VERSION);
    assert_eq!(corpus.cases.len(), 446);
    for case in &corpus.cases {
        compare(case);
    }
}
#[test]
fn composer_has_no_shared_mutable_state_between_parallel_songs() {
    let cases = corpus();
    std::thread::scope(|scope| {
        for group in cases.cases.chunks(80) {
            scope.spawn(move || {
                for case in group {
                    compare(case);
                }
            });
        }
    });
}
#[test]
fn multiple_base_success_missing_material_and_missing_song_aev2_stay_distinct() {
    let multiple = compose::compose(&json!({"baseJson":{"a":{},"b":{}},"diyJson":{}})).unwrap();
    assert!(multiple.merged_json.is_none());
    assert!(
        serde_json::to_value(multiple)
            .unwrap()
            .get("mergedJson")
            .is_none()
    );
    let empty = compose::compose(&json!({"songJson":{},"diyJson":{}})).unwrap();
    assert_eq!(empty.merged_json, Some(json!({})));
    let base = compose::compose(&json!({"baseJson":{},"diyJson":{}})).unwrap();
    assert_eq!(base.merged_json, Some(json!({"aev2":{}})));
}
#[test]
fn bounded_composer_rejects_unsupported_documents_and_nonfinite_materials() {
    for input in [
        json!(null),
        json!({"songJson":[],"diyJson":{}}),
        json!({"songJson":{},"diyJson":[]}),
    ] {
        assert!(compose::compose(&input).is_err());
    }
    assert!(compose::compose(&json!({"songJson":{"aev2":{"cmp":{"pregain":1e308}}},"diyJson":{"en_vibe":{"cmp":{"pregain":1e308}}}})).is_err());
    let many: serde_json::Map<String, Value> =
        (0..257).map(|i| (i.to_string(), json!({}))).collect();
    assert!(compose::compose(&json!({"songJson":{},"diyJson":many})).is_err());
    assert!(
        compose::compose(&json!({"songJson":{},"diyJson":{},"oversized":"x".repeat(4*1024*1024)}))
            .is_err()
    );
}
