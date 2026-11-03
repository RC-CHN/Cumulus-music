use cumulus_core::{Result, effects::material::*};
use serde_json::{Value, json};

fn calls(component: &str, section: &Value) -> Result<Vec<ControlCall>> {
    match component {
        "cmp" => compressor_calls(section),
        "limiter" => limiter_calls(section),
        "dly" => delay_calls(section),
        "alc" => alc_calls(section),
        "spspatial" => speaker_spatial_calls(section),
        _ => panic!("unknown fixture component"),
    }
}

#[test]
fn defaults_types_skipped_objects_and_float_calls_match_native_execution() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/effects-controls-official.json")).unwrap();
    for component in ["cmp", "limiter", "dly", "alc", "spspatial"] {
        for (index, case) in fixture[component].as_array().unwrap().iter().enumerate() {
            let expected: Vec<ControlCall> = serde_json::from_value(case["calls"].clone()).unwrap();
            assert_eq!(
                calls(component, &case["input"]).unwrap(),
                expected,
                "{component} case {index}"
            );
        }
    }
    for case in fixture["routing"].as_array().unwrap() {
        let route = route_sections(&case["input"]).unwrap();
        assert_eq!(
            serde_json::to_value(&route.sections).unwrap(),
            case["routes"]
        );
        let mut actual = vec![json!({"call":"begin"})];
        for section in route.sections {
            actual.extend(
                calls(section.component.key(), section.section)
                    .unwrap()
                    .into_iter()
                    .map(|call| serde_json::to_value(call).unwrap()),
            );
        }
        actual.push(json!({"call":"end"}));
        // Normalize each f32 call through the same typed representation;
        // textual JSON encodings may differ despite identical float values.
        let normalized: Vec<Value> = case["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| {
                if ["begin", "end"].contains(&value["call"].as_str().unwrap()) {
                    value.clone()
                } else {
                    serde_json::to_value(
                        serde_json::from_value::<ControlCall>(value.clone()).unwrap(),
                    )
                    .unwrap()
                }
            })
            .collect();
        assert_eq!(actual, normalized);
    }
}

#[test]
fn skipped_and_explicit_off_are_distinct_and_unsafe_plans_do_not_escape() {
    for component in ["cmp", "limiter", "dly", "alc", "spspatial"] {
        assert!(calls(component, &Value::Null).is_err());
        assert!(calls(component, &json!([])).is_err());
        assert_eq!(calls(component, &json!({"on":false})).unwrap().len(), 1);
    }
    for component in ["dly", "alc", "spspatial"] {
        assert!(calls(component, &json!({})).unwrap().is_empty());
        assert!(calls(component, &json!({"on":1})).unwrap().is_empty());
    }
    for component in ["cmp", "limiter"] {
        assert!(calls(component, &json!({"on":true,"threshold":1e100})).is_err());
        assert_eq!(
            calls(component, &json!({"on":false,"threshold":1e100}))
                .unwrap()
                .len(),
            1
        );
    }
    assert!(delay_calls(&json!({"on":true,"delay":1e100,"feedback":0,"dry":1,"wet":0})).is_err());
    // Required lookup failure precedes native f64 -> f32 conversion.
    assert!(
        delay_calls(&json!({"on":true,"delay":1e100,"feedback":0,"dry":1}))
            .unwrap()
            .is_empty()
    );
}
