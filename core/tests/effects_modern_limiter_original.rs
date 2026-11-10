//! Original x64 SDK output over two synthetic material recipes. No archive or
//! original DLL is required to validate the recovered graph.
#![cfg(all(
    any(target_os = "linux", target_os = "windows"),
    target_arch = "x86_64"
))]

use std::collections::BTreeMap;

use cumulus_core::{
    Error,
    effects::{
        dsp::{Format, LimiterStreamQuantum},
        material::{ModernLimiterPlan, ModernLimiterRenderer},
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Oracle {
    schema: String,
    sdk_sha256: String,
    quantum: u32,
    profiles: BTreeMap<String, Value>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    profile: String,
    rate: u32,
    split: Rendered,
    reset_after_eof: Rendered,
    reset_before_eof: Rendered,
    partial_before_reset: Rendered,
    quanta: Vec<Quantum>,
    late_drain: Rendered,
    late_drain_quanta: Vec<Quantum>,
    empty_source: Rendered,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rendered {
    output_frames: usize,
    output_sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Quantum {
    input_frames: u32,
    input_end: bool,
    output_frames: u32,
    output_end: bool,
    output_sha256: String,
}

fn oracle() -> Oracle {
    serde_json::from_str(include_str!(
        "fixtures/effects-modern-limiter-graph-official.json"
    ))
    .unwrap()
}

fn synth(first: usize, count: usize) -> Vec<f32> {
    (first..first + count)
        .flat_map(|i| {
            [
                if i % 27 == 0 {
                    0.875
                } else {
                    (((i * 37) % 256) as f32 - 128.0) / 512.0
                },
                if i % 29 == 0 {
                    -0.625
                } else {
                    (((i * 53) % 128) as f32 - 64.0) / 512.0
                },
            ]
        })
        .collect()
}

fn hash(pcm: &[f32]) -> String {
    let mut hash = Sha256::new();
    for value in pcm {
        hash.update(value.to_le_bytes());
    }
    hex::encode(hash.finalize())
}

fn push(
    graph: &mut ModernLimiterRenderer,
    samples: &[f32],
    observed: &mut Vec<LimiterStreamQuantum>,
) {
    let receipt = graph
        .push_interleaved_observed(samples, None, |entry| observed.push(entry))
        .unwrap();
    assert_eq!(receipt.accepted_frames as usize, samples.len() / 2);
    assert_eq!(receipt.input_end, samples.is_empty());
}

fn run(
    graph: &mut ModernLimiterRenderer,
    first: usize,
    chunks: &[usize],
    drain_between_writes: bool,
    observed: &mut Vec<LimiterStreamQuantum>,
) -> Vec<f32> {
    let mut source_at = first;
    let mut output = Vec::new();
    let mut output_buffer = [0.0; 4096];
    for &count in chunks {
        push(graph, &synth(source_at, count), observed);
        source_at += count;
        if drain_between_writes {
            let read = graph.pull_interleaved(&mut output_buffer).unwrap();
            assert_eq!(read.end, Some(false));
            output.extend_from_slice(&output_buffer[..read.frames as usize * 2]);
        }
    }
    push(graph, &[], observed);
    for _ in 0..8 {
        let read = graph.pull_interleaved(&mut output_buffer).unwrap();
        output.extend_from_slice(&output_buffer[..read.frames as usize * 2]);
        if read.end == Some(true) {
            let repeat = graph.pull_interleaved(&mut output_buffer).unwrap();
            assert_eq!((repeat.frames, repeat.end), (0, Some(true)));
            assert_eq!(graph.pull_interleaved(&mut []).unwrap().end, None);
            assert_eq!(
                graph
                    .push_interleaved(&[0.5, -0.5], None)
                    .unwrap()
                    .accepted_frames,
                0
            );
            return output;
        }
    }
    panic!("original graph did not drain");
}

fn check_pcm(expected: &Rendered, pcm: &[f32]) {
    assert_eq!(pcm.len(), expected.output_frames * 2);
    assert_eq!(hash(pcm), expected.output_sha256);
}

fn check_quanta(actual: &[LimiterStreamQuantum], expected: &[Quantum], pcm: Option<&[f32]>) {
    assert_eq!(actual.len(), expected.len());
    let mut at = 0;
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual.input_frames, expected.input_frames);
        assert_eq!(actual.input_end, expected.input_end);
        assert_eq!(actual.timing.output_frames, expected.output_frames);
        assert_eq!(actual.timing.output_end, expected.output_end);
        if let Some(pcm) = pcm {
            let end = at + expected.output_frames as usize * 2;
            assert_eq!(hash(&pcm[at..end]), expected.output_sha256);
            at = end;
        }
    }
    if let Some(pcm) = pcm {
        assert_eq!(at, pcm.len());
    }
}

fn reset(graph: &mut ModernLimiterRenderer) {
    graph.reset().unwrap();
    let read = graph.pull_interleaved(&mut [0.0; 256]).unwrap();
    assert_eq!(
        (read.frames, read.output_ready, read.end),
        (0, 0, Some(false))
    );
}

#[test]
fn original_full_graph_pcm_and_fifo_match_two_synthetic_materials() {
    let oracle = oracle();
    assert_eq!(oracle.schema, "cumulus.modern-limiter-x64-pcm.v1");
    assert_eq!(
        oracle.sdk_sha256,
        "c59105e1072d7d7dbd2d2844ac9c170de7e7b41eb463dc56a11c9b37bafac6be"
    );
    assert_eq!(oracle.cases.len(), 4);
    for case in &oracle.cases {
        let format = Format::new(case.rate as f32, 2, oracle.quantum).unwrap();
        let plan = ModernLimiterPlan::from_material(&oracle.profiles[&case.profile]).unwrap();
        let open = || plan.pcm_renderer(format, 8192).unwrap();
        let mut graph = open();
        let mut quanta = Vec::new();
        let fresh = run(
            &mut graph,
            0,
            &[511, 1537, 257, 1791, 2048, 7],
            true,
            &mut quanta,
        );
        check_pcm(&case.split, &fresh);
        check_quanta(&quanta, &case.quanta[..4], Some(&fresh));
        assert_eq!(
            graph.lookahead_frames(),
            2048 - quanta[0].timing.output_frames
        );
        quanta.clear();
        let repacketized = run(&mut open(), 0, &[2048, 2048, 2048, 7], false, &mut quanta);
        check_pcm(&case.split, &repacketized);
        check_quanta(&quanta, &case.quanta[..4], Some(&repacketized));

        reset(&mut graph);
        quanta.clear();
        let after_eof = run(&mut graph, 10_000, &[2048, 2048], true, &mut quanta);
        check_pcm(&case.reset_after_eof, &after_eof);
        check_quanta(&quanta, &case.quanta[4..7], Some(&after_eof));
        assert_eq!(
            quanta[2].input_frames, 1,
            "the source FIFO inserts the sentinel"
        );

        reset(&mut graph);
        quanta.clear();
        let receipt = graph
            .push_interleaved_observed(&synth(20_000, 2048), Some(500), |q| quanta.push(q))
            .unwrap();
        assert!(receipt.output_ready > 31);
        assert_eq!(quanta[0].timestamp_ms, 500);
        let pending = graph.push_interleaved(&synth(22_048, 511), None).unwrap();
        assert_eq!(pending.input_ready, 511);
        let mut partial = [0.0; 62];
        let read = graph.pull_interleaved(&mut partial).unwrap();
        assert_eq!((read.frames, read.end), (31, Some(false)));
        check_pcm(&case.partial_before_reset, &partial);
        check_quanta(&quanta, &case.quanta[7..8], None);
        reset(&mut graph);
        quanta.clear();
        let after_live = run(&mut graph, 30_000, &[2048, 2048, 7], true, &mut quanta);
        check_pcm(&case.reset_before_eof, &after_live);
        check_quanta(&quanta, &case.quanta[8..], Some(&after_live));
        assert_eq!(quanta[0].timestamp_ms, 0, "reset discards the old clock");

        quanta.clear();
        let late = run(&mut open(), 40_000, &[2048, 2043], true, &mut quanta);
        check_pcm(&case.late_drain, &late);
        check_quanta(&quanta, &case.late_drain_quanta, Some(&late));
        assert_eq!(quanta.last().unwrap().input_frames, 0);
        assert!(!quanta[1].timing.output_end);
        assert!(quanta[2].timing.output_end);

        quanta.clear();
        let empty = run(&mut open(), 0, &[], false, &mut quanta);
        check_pcm(&case.empty_source, &empty);
        assert_eq!(quanta.len(), 1);
        assert_eq!(quanta[0].input_frames, 1);
    }
}

#[test]
fn eligibility_rejects_other_graphs_and_cannot_merge_legacy_controls() {
    let oracle = oracle();
    let valid = &oracle.profiles["neutralTone"];
    for (path, replacement) in [
        ("/aev2", json!([])),
        ("/aev2/limiter/on", json!(false)),
        ("/aev2/limiter/infiniteRatio", json!(false)),
        ("/aev2/limiter/attack", json!(0.025)),
        ("/aev2/limiter/release", Value::Null),
        ("/aev2/bt/on", json!(false)),
        ("/aev2/bt/bass", json!("0")),
        ("/aev2/eq/eqs/0", Value::Null),
        ("/aev2/peq/f/1/band", json!(0)),
        ("/aev2/peq/f/0/freq", json!(22_050)),
        ("/aev2/peq/f/1/q", json!(0)),
        ("/aev2/se/on", json!(true)),
        ("/aev2/rvb/on", json!("off")),
        ("/aev2/cmp/on", json!(1)),
    ] {
        let mut bad = valid.clone();
        *bad.pointer_mut(path).unwrap() = replacement;
        assert!(ModernLimiterPlan::from_material(&bad).is_err(), "{path}");
    }
    let mut extra = valid.clone();
    extra["aev2"]["rotate"] = json!({"on": false});
    assert!(ModernLimiterPlan::from_material(&extra).is_err());
    let mut missing = valid.clone();
    missing["aev2"].as_object_mut().unwrap().remove("limiter");
    missing["limiter"] = valid["aev2"]["limiter"].clone();
    assert!(ModernLimiterPlan::from_material(&missing).is_err());
    assert!(ModernLimiterPlan::from_material(&valid["aev2"]).is_err());

    let mut legacy = valid.clone();
    legacy["eq"] = json!({"on": false});
    legacy["se"] = json!({"on": true});
    legacy["limiter"] = json!({"on": false});
    let plan = ModernLimiterPlan::from_material(&legacy).unwrap();
    for (rate, channels, quantum) in [
        (44_100.0, 1, 2048),
        (48_000.0, 6, 2048),
        (96_000.0, 2, 2048),
        (48_000.0, 2, 512),
    ] {
        let format = Format::new(rate, channels, quantum).unwrap();
        assert!(plan.pcm_renderer(format, 8192).is_err());
    }
    let format = Format::new(44_100.0, 2, 2048).unwrap();
    assert!(plan.pcm_renderer(format, 2047).is_err());
    assert!(plan.pcm_renderer(format, 65_537).is_err());
    let mut graph = plan.pcm_renderer(format, 8192).unwrap();
    let pcm = run(
        &mut graph,
        0,
        &[2048, 2048, 2048, 7],
        false,
        &mut Vec::new(),
    );
    check_pcm(&oracle.cases[0].split, &pcm);
}

#[test]
fn invalid_or_backpressured_writes_are_atomic_but_kernel_failure_quarantines() {
    let oracle = oracle();
    let plan = ModernLimiterPlan::from_material(&oracle.profiles["neutralTone"]).unwrap();
    let format = Format::new(44_100.0, 2, 2048).unwrap();
    let mut graph = plan.pcm_renderer(format, 2048).unwrap();
    assert!(matches!(
        graph.push_interleaved(&[0.1], None),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        graph.push_interleaved(&[f32::NAN, 0.0], None),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        graph.push_interleaved(&[0.1, -0.2], Some(u32::MAX)),
        Err(Error::InvalidInput(_))
    ));
    let first = synth(0, 2048);
    let before = graph.push_interleaved(&first, Some(7)).unwrap();
    let next = synth(2048, 2048);
    assert!(matches!(
        graph.push_interleaved(&next, Some(600)),
        Err(Error::State(_))
    ));
    let mut scratch = [0.0; 4096];
    let read = graph.pull_interleaved(&mut scratch).unwrap();
    assert_eq!(read.frames, before.output_ready);
    assert_eq!(
        hash(&scratch[..read.frames as usize * 2]),
        oracle.cases[0].quanta[0].output_sha256
    );
    let mut retried_clock = None;
    graph
        .push_interleaved_observed(&next, None, |entry| {
            retried_clock = Some(entry.timestamp_ms)
        })
        .unwrap();
    assert_eq!(retried_clock, before.clock_ms);

    let mut graph = plan.pcm_renderer(format, 8192).unwrap();
    assert!(graph.push_interleaved(&first, None).unwrap().output_ready > 0);
    let bad = [f32::MAX, -f32::MAX].repeat(2048);
    assert!(graph.push_interleaved(&bad, None).is_err());
    let mut untouched = [0.125; 32];
    assert!(matches!(
        graph.pull_interleaved(&mut untouched),
        Err(Error::State(_))
    ));
    assert_eq!(untouched, [0.125; 32]);
    assert!(matches!(graph.reset(), Err(Error::State(_))));
    assert!(matches!(
        graph.push_interleaved(&[], None),
        Err(Error::State(_))
    ));
}
