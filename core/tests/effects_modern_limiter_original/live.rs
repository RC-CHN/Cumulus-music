use super::{Format, LimiterStreamQuantum, ModernLimiterPlan, hash, json, oracle, synth};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Corpus {
    schema: String,
    sdk_sha256: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    profile: String,
    rate: u32,
    mode: Mode,
    inputs: Vec<u32>,
    outputs: Vec<u32>,
    frames: [usize; 3],
    sha256: [String; 3],
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Steady,
    Switch,
    Coalesced,
}

#[test]
fn original_live_controls_preserve_outer_fifos_and_reprime_only_b0() {
    let fixtures: Corpus = serde_json::from_str(include_str!(
        "../fixtures/effects-modern-limiter-switch-official.json"
    ))
    .unwrap();
    assert_eq!(fixtures.schema, "cumulus.modern-limiter-x64-hot.v1");
    let baseline = oracle();
    assert_eq!(fixtures.sdk_sha256, baseline.sdk_sha256);
    assert_eq!(fixtures.cases.len(), 24);

    for case in fixtures.cases {
        let base = match case.profile.as_str() {
            "defaultTiming" | "releaseOnlyTiming" => "nonzeroTone",
            name => name,
        };
        let mut profile = baseline.profiles[base].clone();
        if case.profile == "defaultTiming" {
            profile["aev2"]["limiter"] = json!({
                "on": true, "threshold": 0, "attack": 0.005, "release": 0.1,
                "outputgain": 0, "infiniteRatio": true, "ratio": 4
            });
        }
        if case.profile == "releaseOnlyTiming" {
            profile["aev2"]["limiter"]["attack"] = json!(0.005);
        }
        let format = Format::new(case.rate as f32, 2, baseline.quantum).unwrap();
        let mut graph = ModernLimiterPlan::from_material(&profile)
            .unwrap()
            .pcm_renderer(format, 4096)
            .unwrap();
        let mut quanta = Vec::new();
        let mut push = |graph: &mut super::ModernLimiterRenderer, first, count, stamp| {
            graph
                .push_interleaved_observed(&synth(first, count), stamp, |q| quanta.push(q))
                .unwrap()
        };
        let pull = |graph: &mut super::ModernLimiterRenderer, frames: usize, expected_end| {
            let mut pcm = vec![0.0; frames * 2];
            let read = graph.pull_interleaved(&mut pcm).unwrap();
            assert_eq!(read.end, expected_end);
            pcm.truncate(read.frames as usize * 2);
            pcm
        };

        push(&mut graph, 0, 2048, Some(1000));
        let mut old = pull(&mut graph, 31, Some(false));
        let pending = push(&mut graph, 2048, 511, None);
        assert_eq!(pending.input_ready, 511);
        assert_eq!(pending.output_ready as usize, case.frames[0] - 31);
        assert!(pending.clock_ms.unwrap() > 1000);
        if case.mode != Mode::Steady {
            graph.set_material_active(false).unwrap();
        }
        old.extend(pull(&mut graph, 2048, Some(false)));
        if case.mode == Mode::Coalesced {
            graph.set_material_active(true).unwrap();
        }
        push(&mut graph, 2559, 1537, None);
        let mut middle = pull(&mut graph, 31, Some(false));
        let next = push(&mut graph, 4096, 257, None);
        assert_eq!(next.input_ready, 257);
        assert_eq!(next.output_ready as usize, case.frames[1] - 31);
        assert!(next.clock_ms > pending.clock_ms);
        if case.mode == Mode::Switch {
            graph.set_material_active(true).unwrap();
        }
        middle.extend(pull(&mut graph, 2048, Some(false)));
        push(&mut graph, 4353, 1791, None);
        let mut tail = pull(&mut graph, 2048, Some(false));
        push(&mut graph, 6144, 7, None);
        push(&mut graph, 6151, 0, None);
        assert!(graph.set_material_active(false).is_err());
        tail.extend(pull(&mut graph, 2048, Some(true)));
        assert!(pull(&mut graph, 2048, Some(true)).is_empty());
        let label = (&case.profile, case.rate, case.mode);
        assert_eq!(
            quanta
                .iter()
                .map(|q: &LimiterStreamQuantum| q.input_frames)
                .collect::<Vec<_>>(),
            case.inputs,
            "{label:?} input quanta"
        );
        assert_eq!(
            quanta
                .iter()
                .map(|q| q.timing.output_frames)
                .collect::<Vec<_>>(),
            case.outputs,
            "{label:?} output quanta"
        );
        assert_eq!(
            quanta.iter().map(|q| q.timestamp_ms).collect::<Vec<_>>(),
            if case.rate == 44100 {
                [1000, 1046, 1092, 1138]
            } else {
                [1000, 1042, 1084, 1126]
            },
            "{label:?} source clock"
        );
        for (index, pcm) in [old, middle, tail].into_iter().enumerate() {
            assert_eq!(
                pcm.len() / 2,
                case.frames[index],
                "{label:?} segment {index}"
            );
            assert_eq!(hash(&pcm), case.sha256[index], "{label:?} segment {index}");
        }
    }
}

#[test]
fn unsupported_gain_only_switch_leaves_an_active_stream_unchanged() {
    let fixtures = oracle();
    let mut profile = fixtures.profiles["nonzeroTone"].clone();
    profile["aev2"]["limiter"]["attack"] = json!(0.005);
    profile["aev2"]["limiter"]["release"] = json!(0.1);
    let plan = ModernLimiterPlan::from_material(&profile).unwrap();
    let format = Format::new(44_100.0, 2, 2048).unwrap();
    let render = |try_switch| {
        let mut graph = plan.pcm_renderer(format, 4096).unwrap();
        let mut out = Vec::new();
        let mut buffer = [0.0; 4096];
        for (start, count) in [(0, 2048), (2048, 511), (2559, 1537), (4096, 7), (4103, 0)] {
            graph.push_interleaved(&synth(start, count), None).unwrap();
            if start == 2048 && try_switch {
                assert!(matches!(
                    graph.set_material_active(false),
                    Err(cumulus_core::Error::InvalidInput(_))
                ));
            }
            let result = graph.pull_interleaved(&mut buffer).unwrap();
            out.extend_from_slice(&buffer[..result.frames as usize * 2]);
        }
        out
    };
    assert_eq!(render(true), render(false));
}
