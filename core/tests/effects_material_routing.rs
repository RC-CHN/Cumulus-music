use cumulus_core::effects::material::{
    EqualizationCall, MaterialSchema, bass_treble_calls, graphic_calls, route_sections,
};
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/effects-routing-official.json")).unwrap()
}

#[test]
fn complete_branch_and_component_order_match_official_native_calls() {
    let fixtures = fixture();
    for (index, case) in fixtures["routing"].as_array().unwrap().iter().enumerate() {
        let input = &case["input"];
        let route = route_sections(input).unwrap();
        assert_eq!(
            route.schema == MaterialSchema::Version2,
            input["aev2"].is_object()
        );
        assert_eq!(
            serde_json::to_value(&route.sections).unwrap(),
            case["routes"],
            "case {index}"
        );
        let mut calls = vec![json!({"call":"begin"})];
        for section in route.sections {
            let parsed = match section.component.key() {
                "eq" => graphic_calls(section.section).unwrap(),
                "bt" => bass_treble_calls(section.section).unwrap(),
                _ => continue, // Other component bodies were host seams in this oracle.
            };
            calls.extend(
                parsed
                    .into_iter()
                    .map(|value| serde_json::to_value(value).unwrap()),
            );
        }
        calls.push(json!({"call":"end"}));
        assert_eq!(calls, *case["calls"].as_array().unwrap(), "case {index}");
    }
}

#[test]
fn equalization_calls_match_native_defaults_short_arrays_types_and_float_conversion() {
    let fixtures = fixture();
    for kind in ["eq", "bt"] {
        for (index, case) in fixtures[kind].as_array().unwrap().iter().enumerate() {
            let expected: Vec<EqualizationCall> =
                serde_json::from_value(case["calls"].clone()).unwrap();
            let actual = if kind == "eq" {
                graphic_calls(&case["input"])
            } else {
                bass_treble_calls(&case["input"])
            }
            .unwrap();
            assert_eq!(actual, expected, "{kind} case {index}");
        }
    }
}

#[test]
fn malformed_roots_and_nonrepresentable_float_parameters_do_not_produce_partial_plans() {
    for input in [Value::Null, json!([]), json!(true), json!("{}"), json!(3)] {
        assert!(route_sections(&input).is_err());
        assert!(graphic_calls(&input).is_err());
        assert!(bass_treble_calls(&input).is_err());
    }
    assert!(graphic_calls(&json!({"on":true,"eqs":[2,1e100]})).is_err());
    assert!(bass_treble_calls(&json!({"on":true,"bass":2,"treble":1e100})).is_err());
    // The native disabled path does not interpret irrelevant gain fields.
    assert_eq!(
        graphic_calls(&json!({"on":false,"eqs":[1e100]})).unwrap(),
        vec![EqualizationCall::EqOn { value: false }]
    );
    assert_eq!(
        bass_treble_calls(&json!({"on":false,"bass":1e100})).unwrap(),
        vec![EqualizationCall::BtOn { value: false }]
    );
}
