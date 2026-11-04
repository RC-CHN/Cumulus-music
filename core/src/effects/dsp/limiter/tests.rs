use super::{Envelope, Format, LimiterSettings, LimiterStage, PlanarBlock};
use crate::effects::material::material_limiter_plan;
use serde_json::{Value, json};

fn vectors() -> Value {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/effects-limiter-kernel-official.json"
    ))
    .unwrap()
}

fn u32_value(value: &Value) -> u32 {
    u32::try_from(value.as_u64().unwrap()).unwrap()
}

fn format(case: &Value) -> Format {
    Format::new(
        u32_value(&case["rate"]) as f32,
        u32_value(&case["channels"]),
        u32_value(&case["capacity"]),
    )
    .unwrap()
}

fn settings(case: &Value) -> LimiterSettings {
    let params = &case["params"];
    let number = |key: &str, default| params[key].as_f64().map_or(default, |x| x as f32);
    LimiterSettings {
        enabled: true,
        threshold_db: number("threshold", 0.0),
        attack_seconds: number("attack", 0.005),
        release_seconds: number("release", 0.1),
        input_gain_db: number("inputgain", 0.0),
        output_gain_db: number("outputgain", 0.0),
        infinite_ratio: params["infinite_ratio"].as_bool().unwrap_or(true),
        ratio: number("ratio", 4.0),
    }
}

fn input(block: &Value) -> Vec<f32> {
    block["input"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|frame| frame.as_array().unwrap().iter())
        .map(|sample| sample.as_f64().unwrap() as f32)
        .collect()
}

#[test]
fn linked_envelope_intermediates_match_native_bits() {
    for case in vectors()["cases"].as_array().unwrap() {
        let trace = case["trace"].as_array().unwrap();
        if trace.is_empty() {
            continue;
        }
        let settings = settings(case);
        let format = format(case);
        let channels = format.channels();
        let configuration = &case["configuration"];
        assert_eq!(configuration["normalizeByThreshold"], false);
        let window = u32_value(&configuration["linkedWindow"]) as usize;
        let mut stage = Envelope::new(format, window, settings);
        let mut zero_budget = window - 1;
        let mut index = 0;
        for block in case["blocks"].as_array().unwrap() {
            let mut samples = input(block);
            if block["inEnd"].as_bool().unwrap() {
                let padding = zero_budget.min(format.capacity() - samples.len() / channels);
                zero_budget -= padding;
                samples.resize(samples.len() + padding * channels, 0.0);
            }
            for frame in samples.chunks_exact(channels) {
                let native = trace[index].as_array().unwrap();
                let mut pcm = [0.0; 8];
                let (peak, gain) = stage.step(frame, &mut pcm[..channels]);
                let (next_gain, slope, count, peak_count, peak_sum) = stage.state();
                let actual = [peak, gain, next_gain, slope];
                for (value, expected) in actual
                    .into_iter()
                    .chain([peak_sum])
                    .chain(pcm[..channels].iter().copied())
                    .zip(native[..4].iter().chain(&native[6..]))
                {
                    let bits = u64::from_str_radix(expected.as_str().unwrap(), 16).unwrap();
                    assert_eq!(
                        value.to_bits(),
                        bits,
                        "{} frame {index}: {value:?} != {:?}",
                        case["name"],
                        f64::from_bits(bits)
                    );
                }
                assert_eq!(
                    count,
                    u32_value(&native[4]) as usize,
                    "{} frame {index}",
                    case["name"]
                );
                assert_eq!(
                    peak_count,
                    u32_value(&native[5]),
                    "{} frame {index}",
                    case["name"]
                );
                index += 1;
            }
        }
        assert_eq!(index, trace.len());
    }
}

#[test]
fn native_pcm_and_variable_output_match_through_every_block() {
    for case in vectors()["cases"].as_array().unwrap() {
        let format = format(case);
        let mut stage = match case["name"].as_str().unwrap() {
            "default-window" => material_limiter_plan(&json!({}))
                .unwrap()
                .pcm_stage(format)
                .unwrap(),
            "eight-channel-simd" => {
                let mut limiter = case["params"].clone();
                limiter["on"] = json!(true);
                material_limiter_plan(&json!({"aev2":{"limiter":limiter}}))
                    .unwrap()
                    .pcm_stage(format)
                    .unwrap()
            }
            _ => LimiterStage::new(format, settings(case)).unwrap(),
        };
        assert_eq!(
            stage.lookahead_frames() + 1,
            u32_value(&case["configuration"]["linkedWindow"])
        );
        let mut block = PlanarBlock::new(format);
        let capacity = format.capacity();
        for native in case["blocks"].as_array().unwrap() {
            let frames = block.load_interleaved(&input(native)).unwrap();
            assert_eq!(frames, u32_value(&native["inFrames"]) as usize);
            let output = stage
                .process(
                    &mut block,
                    frames as u32,
                    native["inEnd"].as_bool().unwrap(),
                )
                .unwrap();
            assert_eq!(output.output_frames, u32_value(&native["outFrames"]));
            assert_eq!(output.output_end, native["outEnd"].as_bool().unwrap());
            for (channel, samples) in native["samples"].as_array().unwrap().iter().enumerate() {
                for (frame, sample) in samples.as_array().unwrap().iter().enumerate() {
                    let expected = sample.as_f64().unwrap() as f32;
                    let actual = block.samples()[channel * capacity + frame];
                    assert_eq!(
                        actual.to_bits(),
                        expected.to_bits(),
                        "{} ch{channel} frame{frame}",
                        case["name"]
                    );
                }
            }
        }
    }
}

#[test]
fn unsupported_input_does_not_consume_the_lookahead_window() {
    let format = Format::new(48_000.0, 2, 256).unwrap();
    let plan = material_limiter_plan(&json!({})).unwrap();
    let mut stage = plan.pcm_stage(format).unwrap();
    let mut block = PlanarBlock::new(format);
    block.samples_mut()[0] = f32::NAN;
    assert!(stage.process(&mut block, 8, false).is_err());
    block.samples_mut()[0] = 0.0;
    let timing = stage.process(&mut block, 256, false).unwrap();
    assert_eq!(timing.output_frames, 18);
    assert_eq!(timing.skip_output_frames, 238);
}
