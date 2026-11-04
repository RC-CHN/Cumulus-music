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

#[test]
fn limiter_material_begin_and_explicit_overrides_match_both_native_traces() {
    let graph: Value =
        serde_json::from_str(include_str!("fixtures/effects-limiter-pcm-official.json")).unwrap();
    let begin = graph["beginEvents"].as_array().unwrap();
    let native = &begin[2];
    let defaults: LimiterParameters = serde_json::from_value(native.clone()).unwrap();
    let expected_begin = [
        LimiterBeginCall::ResetBypass {
            value: begin[0]["value"].as_bool().unwrap(),
        },
        LimiterBeginCall::MasterBypassReset {
            value: begin[1]["value"].as_bool().unwrap(),
        },
        LimiterBeginCall::Parameters {
            parameters: defaults,
            flag_48: native["flag48"].as_bool().unwrap(),
        },
    ];
    for (material, schema) in [
        (json!({}), MaterialSchema::Legacy),
        (json!({"limiter":{"on":false}}), MaterialSchema::Legacy),
        (
            json!({"limiter":{"on":false},"aev2":{}}),
            MaterialSchema::Version2,
        ),
        (json!({"aev2":{"limiter":false}}), MaterialSchema::Version2),
    ] {
        let plan = material_limiter_plan(&material).unwrap();
        assert_eq!(plan.schema, schema);
        assert_eq!(plan.begin, expected_begin);
        assert!(plan.section.is_empty() && plan.enabled);
        assert_eq!(plan.parameters, defaults);
    }

    let explicit: Value =
        serde_json::from_str(include_str!("fixtures/effects-controls-official.json")).unwrap();
    for case in explicit["limiter"].as_array().unwrap() {
        let expected: Vec<ControlCall> = serde_json::from_value(case["calls"].clone()).unwrap();
        let plan = material_limiter_plan(&json!({"aev2":{"limiter":case["input"]}})).unwrap();
        assert_eq!(plan.schema, MaterialSchema::Version2);
        assert_eq!(plan.begin, expected_begin);
        assert_eq!(plan.section, expected);
        match expected.as_slice() {
            [
                ControlCall::LimiterParameters { parameters },
                ControlCall::LimiterOn { value: true },
            ] => {
                assert!(plan.enabled);
                assert_eq!(plan.parameters, *parameters);
            }
            [ControlCall::LimiterOn { value: false }] => {
                assert!(!plan.enabled);
                assert_eq!(plan.parameters, defaults);
            }
            other => panic!("unexpected native limiter sequence: {other:?}"),
        }
    }
    assert!(
        material_limiter_plan(&json!({"aev2":{"limiter":{"on":true,"attack":1e100}}})).is_err()
    );
}

#[test]
fn limiter_block_capacity_startup_and_eof_match_the_native_pcm_node() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/effects-limiter-pcm-official.json")).unwrap();
    let number = |value: &Value| u32::try_from(value.as_u64().unwrap()).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let material = match (
            case["enabled"].as_bool().unwrap(),
            case["thresholdDb"].as_f64().unwrap(),
        ) {
            (false, _) => json!({"aev2":{"limiter":{"on":false}}}),
            (true, 0.0) => json!({}),
            (true, threshold) => {
                json!({"aev2":{"limiter":{"on":true,"threshold":threshold,"attack":case["attackSeconds"]}}})
            }
        };
        let plan = material_limiter_plan(&material).unwrap();
        let capacity = number(&case["maxFrames"]);
        let mut flow = plan
            .frame_flow(number(&case["rate"]), number(&case["channels"]), capacity)
            .unwrap();
        assert_eq!(flow.lookahead_frames(), number(&case["lookaheadFrames"]));
        assert!(flow.next_block(capacity + 1, true).is_err());
        for block in case["blocks"].as_array().unwrap() {
            let observed = flow
                .next_block(
                    number(&block["inFrames"]),
                    block["inEnd"].as_bool().unwrap(),
                )
                .unwrap();
            assert_eq!(
                observed.output_frames,
                number(&block["outFrames"]),
                "{case}"
            );
            assert_eq!(
                observed.output_end,
                block["outEnd"].as_bool().unwrap(),
                "{case}"
            );
        }
    }

    for attack in [0.000001, 0.11] {
        let plan = material_limiter_plan(&json!({"aev2":{"limiter":{"on":true,"attack":attack}}}))
            .unwrap();
        assert!(plan.frame_flow(48_000, 2, 128).is_err());
    }
}
