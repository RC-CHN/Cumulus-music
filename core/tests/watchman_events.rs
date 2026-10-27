use cumulus_core::watchman::{descriptors, encode_field, events::*};
use serde_json::{Value, json};
fn fixtures() -> Vec<Value> {
    [
        include_str!("fixtures/watchman-events-official.json"),
        include_str!("fixtures/watchman-events-legacy-official.json"),
    ]
    .into_iter()
    .map(|v| serde_json::from_str(v).unwrap())
    .collect()
}

#[test]
fn official_event_methods_match_sampling_bytes_and_clear_boundaries() {
    let table = descriptors().unwrap();
    for fixture in fixtures() {
        for (case_index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let policy =
                EventPolicy::from_options(&case["options"], case["cors"].as_bool().unwrap());
            for (name, kind) in [
                ("move", EventKind::Move),
                ("down", EventKind::Down),
                ("up", EventKind::Up),
                ("click", EventKind::Click),
                ("keydown", EventKind::Keydown),
                ("focus", EventKind::Focus),
                ("blur", EventKind::Blur),
                ("scroll", EventKind::Scroll),
                ("orientation", EventKind::Orientation),
                ("motion", EventKind::Motion),
                ("battery", EventKind::Battery),
            ] {
                let limit = policy.limit(kind);
                assert_eq!(
                    limit.maximum,
                    case["limits"][format!("{name}Max")]
                        .as_f64()
                        .unwrap()
                        .ceil() as usize
                );
                assert_eq!(
                    limit.interval_ms,
                    case["limits"][format!("{name}Interval")]
                        .as_f64()
                        .unwrap_or(f64::INFINITY)
                );
            }
            let mut sampler = EventSampler::new(policy).unwrap();
            sampler.start(1000).unwrap();
            let mut clock = 1000;
            for (index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
                if let Some(now) = step["at"].as_u64() {
                    clock = now;
                }
                match step["op"].as_str().unwrap() {
                    "event" => {
                        let event: EventSample =
                            serde_json::from_value(step["sample"].clone()).unwrap();
                        let mut failed = false;
                        for i in 0..step["repeat"].as_u64().unwrap_or(1) {
                            if i > 0 {
                                clock += step["spacing"].as_u64().unwrap();
                            }
                            failed |= sampler.record(&event, clock).is_err();
                        }
                        assert_eq!(
                            failed,
                            step["error"].as_bool().unwrap_or(false),
                            "case {case_index} step {index} error"
                        );
                    }
                    "battery" | "battery_init" => {
                        let sample: BatterySample =
                            serde_json::from_value(step["sample"].clone()).unwrap();
                        let random = step["random"].as_f64().unwrap_or(0.25);
                        if step["op"] == "battery_init" {
                            sampler.initialize_battery(sample, clock, random).unwrap();
                        } else {
                            sampler.record_battery(&sample, clock, random).unwrap();
                        }
                    }
                    "reference" => sampler.set_reference(clock).unwrap(),
                    "start" => sampler.start(clock).unwrap(),
                    "stop" => sampler.stop(),
                    "clear" => sampler.clear(),
                    "drain" => {
                        let mut actual: Vec<_> = sampler
                            .drain()
                            .iter()
                            .map(|s| {
                                hex::encode(
                                    encode_field(
                                        table.iter().find(|f| f.key == s.key).unwrap(),
                                        &s.value,
                                        s.failed,
                                    )
                                    .unwrap(),
                                )
                            })
                            .collect();
                        actual.sort();
                        assert_eq!(
                            json!(actual),
                            step["fields"],
                            "case {case_index} step {index} fields"
                        );
                    }
                    _ => panic!("unknown fixture step"),
                }
                assert_eq!(
                    sampler.buffered() as u64,
                    step["buffered"].as_u64().unwrap(),
                    "case {case_index} step {index} buffered"
                );
            }
        }
    }
}

#[test]
fn typed_event_boundary_rejects_keyboard_content_and_stops_late_capture() {
    for field in ["key", "key_code", "value", "password", "cookie"] {
        let mut sample = json!({"dom_type":"keydown","target_id":"login"});
        sample[field] = json!("never capture");
        assert!(serde_json::from_value::<EventSample>(sample).is_err());
    }
    let sample: EventSample =
        serde_json::from_value(json!({"dom_type":"click","target_id":"button"})).unwrap();
    let mut sampler = EventSampler::new(EventPolicy::from_options(&json!({}), true)).unwrap();
    assert_eq!(
        sampler.record(&sample, 1000).unwrap(),
        SampleOutcome::Stopped
    );
    sampler.start(1000).unwrap();
    assert_eq!(
        sampler.record(&sample, 1100).unwrap(),
        SampleOutcome::Stored
    );
    sampler.stop();
    assert_eq!(sampler.buffered(), 0);
    assert_eq!(
        sampler.record(&sample, 1200).unwrap(),
        SampleOutcome::Stopped
    );
    assert!(sampler.start(u64::MAX).is_err());
    sampler.start(2000).unwrap();
    let huge: EventSample =
        serde_json::from_value(json!({"dom_type":"click","target_id":"x".repeat(4097)})).unwrap();
    assert!(sampler.record(&huge, 2100).is_err());
    assert_eq!(sampler.buffered(), 0);
    assert!(
        sampler
            .record_battery(&BatterySample::default(), 2200, 1.)
            .is_err()
    );
}
