use crate::effects::{dsp::Format, material::material_limiter_plan};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn u32_value(value: &Value) -> u32 {
    u32::try_from(value.as_u64().unwrap()).unwrap()
}

fn fixture_pcm(start: u32, count: u32, channels: u32) -> Vec<f32> {
    (start..start + count)
        .flat_map(|i| {
            (0..channels).map(move |ch| {
                let positive = if (i + ch).is_multiple_of(7) {
                    2.0
                } else {
                    0.125 + f64::from(i % 5) / 32.0
                };
                let sign = if (i + ch).is_multiple_of(2) {
                    1.0
                } else {
                    -1.0
                };
                (sign * positive / f64::from(ch + 1)) as f32
            })
        })
        .collect()
}

#[test]
fn bounded_stream_matches_original_fifo_graph_and_pcm() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/effects-limiter-stream-official.json"
    ))
    .unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let channels = u32_value(&case["channels"]);
        let format = Format::new(
            u32_value(&case["rate"]) as f32,
            channels,
            u32_value(&case["quantum"]),
        )
        .unwrap();
        let material = if case["enabled"] == true {
            json!({})
        } else {
            json!({"aev2":{"limiter":{"on":false}}})
        };
        let mut stream = material_limiter_plan(&material)
            .unwrap()
            .pcm_stream(format, u32_value(&case["fifoCapacity"]))
            .unwrap();
        let mut input_end = false;
        for (index, operation) in case["operations"].as_array().unwrap().iter().enumerate() {
            if operation["op"] == "push" {
                let count = u32_value(&operation["frameCount"]);
                let clock = serde_json::from_value(operation["timestampMs"].clone()).unwrap();
                let samples = fixture_pcm(u32_value(&operation["firstFrame"]), count, channels);
                let mut graph = Vec::new();
                let result = stream
                    .push_interleaved_observed(&samples, clock, |next| {
                        graph.push(json!({
                            "inputFrames":next.input_frames, "inputEnd":next.input_end,
                            "outputFrames":next.timing.output_frames, "outputEnd":next.timing.output_end,
                            "timestampMs":next.timestamp_ms
                        }));
                    })
                    .unwrap();
                assert_eq!(result.accepted_frames, if input_end { 0 } else { count });
                assert_eq!(result.input_ready, u32_value(&operation["inputReady"]));
                assert_eq!(result.output_ready, u32_value(&operation["outputReady"]));
                assert_eq!(result.input_end, operation["inputEnd"].as_bool().unwrap());
                assert_eq!(
                    result.clock_ms.map_or(-1, i64::from),
                    operation["clockMs"].as_i64().unwrap()
                );
                assert_eq!(
                    json!(graph),
                    operation["graphCalls"],
                    "{} op {index}",
                    case["name"]
                );
                input_end = result.input_end;
            } else {
                let capacity = u32_value(&operation["capacity"]);
                let mut output = vec![f32::MAX; (capacity * channels) as usize];
                let result = stream.pull_interleaved(&mut output).unwrap();
                assert_eq!(result.frames, u32_value(&operation["frameCount"]));
                assert_eq!(result.output_ready, u32_value(&operation["outputReady"]));
                assert_eq!(json!(result.end), operation["end"]);
                let used = (result.frames * channels) as usize;
                assert!(output[used..].iter().all(|sample| *sample == f32::MAX));
                let bytes: Vec<u8> = output[..used]
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect();
                assert_eq!(
                    hex::encode(Sha256::digest(bytes)),
                    operation["pcmSha256"].as_str().unwrap(),
                    "{} op {index}",
                    case["name"]
                );
            }
        }
    }
}

#[test]
fn output_backpressure_preserves_pcm_timestamp_and_eof_on_retry() {
    let format = Format::new(8_000.0, 1, 4).unwrap();
    let mut stream = material_limiter_plan(&json!({"aev2":{"limiter":{"on":false}}}))
        .unwrap()
        .pcm_stream(format, 8)
        .unwrap();
    let before = stream.push_interleaved(&[0.25; 8], Some(1)).unwrap();
    assert_eq!(before.output_ready, 8);
    assert!(stream.push_interleaved(&[0.75; 4], Some(20)).is_err());
    assert!(stream.push_interleaved(&[], Some(100)).is_err());

    let mut prefix = [0.0; 4];
    let partial = stream.pull_interleaved(&mut prefix).unwrap();
    assert_eq!(partial.end, Some(false));
    assert_eq!(prefix, [0.25; 4]);

    let ended = stream.push_interleaved(&[], None).unwrap();
    assert_eq!(ended.clock_ms, before.clock_ms);
    assert_eq!(ended.output_ready, 5);
    assert!(ended.input_end);
    let mut remaining = [1.0; 5];
    let final_read = stream.pull_interleaved(&mut remaining).unwrap();
    assert_eq!(final_read.end, Some(true));
    assert_eq!(remaining, [0.25, 0.25, 0.25, 0.25, 0.0]);
}
