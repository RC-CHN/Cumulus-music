//! Source-clock/backpressure tests for the bounded subgraph transport.
use cumulus_core::effects::{
    dsp::{Format, PlanarBlock},
    stereo::{LocalStereoMaterialPlan, LocalStereoStream, LocalStereoSubgraph},
};
use serde_json::json;
#[path = "../examples/support/stereo_material_wire.rs"]
mod wire;
fn plan() -> LocalStereoMaterialPlan {
    LocalStereoMaterialPlan::from_equalizer_json(&json!({"eq":{"on":false,"eqs":vec![0;10]},"bt":{"on":false,"bass":0,"treble":0},"rvb":{"on":false},"se":{"on":true,"presence":3,"stereoizer":5,"sshaper":true,"ambience":1}})).unwrap()
}
fn samples(frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|i| {
            [
                ((i % 31) as f32 - 15.0) * 0.0078125,
                ((i % 17) as f32 - 8.0) * 0.015625,
            ]
        })
        .collect()
}
fn bits(pcm: &[f32]) -> Vec<u32> {
    pcm.iter().map(|x| x.to_bits()).collect()
}
fn reference(graph: &mut LocalStereoSubgraph, input: &[f32]) -> Vec<f32> {
    let mut output = Vec::new();
    let mut pcm = PlanarBlock::new(graph.format());
    let mut offset = 0;
    while input.len() - offset >= 4096 {
        pcm.load_interleaved(&input[offset..offset + 4096]).unwrap();
        let flow = graph.process(&mut pcm, 2048, false).unwrap();
        let mut part = vec![0.0; 2 * flow.valid_frames];
        pcm.copy_interleaved(flow.valid_frames, &mut part).unwrap();
        output.extend(part);
        offset += 4096;
    }
    let tail = &input[offset..];
    let sentinel = [0.0, 0.0];
    let frames = pcm
        .load_interleaved(if tail.is_empty() { &sentinel } else { tail })
        .unwrap();
    for index in 0..30 {
        let count = if index == 0 {
            frames
        } else {
            pcm.samples_mut().fill(0.0);
            0
        };
        let flow = graph.process(&mut pcm, count, true).unwrap();
        let mut part = vec![0.0; 2 * flow.valid_frames];
        pcm.copy_interleaved(flow.valid_frames, &mut part).unwrap();
        output.extend(part);
        if flow.end {
            return output;
        }
    }
    panic!("bounded original tail did not finish");
}
fn drain(stream: &mut LocalStereoStream) -> Vec<f32> {
    let mut output = Vec::new();
    let mut part = [0.0; 154];
    for _ in 0..2048 {
        let result = stream.pull_interleaved(&mut part).unwrap();
        output.extend_from_slice(&part[..2 * result.frames as usize]);
        if result.end == Some(true) {
            return output;
        }
        assert!(
            result.frames > 0,
            "no pending decoder input during terminal drain"
        );
    }
    panic!("bounded FIFO did not drain");
}
#[test]
fn packets_reassemble_exact_quantum_keep_real_tail_and_exclude_tail_from_source_clock() {
    for frames in [4096, 5000] {
        let input = samples(frames);
        let mut stream = LocalStereoStream::new(48000, 2, plan(), 131072).unwrap();
        let mut offset = 0;
        for packet in [3, 2048, 1, 933, 2048] {
            let end = (offset + packet).min(frames);
            if end > offset {
                stream
                    .push_interleaved(&input[2 * offset..2 * end], (offset == 0).then_some(1000))
                    .unwrap();
                offset = end;
            }
        }
        if offset < frames {
            stream.push_interleaved(&input[2 * offset..], None).unwrap();
        }
        let end = stream.push_interleaved(&[], None).unwrap();
        assert_eq!(end.clock_ms, Some(if frames == 4096 { 1084 } else { 1102 }));
        // The official input-end latch ignores later writes before validating
        // their invalid payload/timestamp, rather than replaying a terminal DSP.
        assert_eq!(
            stream
                .push_interleaved(&[f32::NAN], Some(u32::MAX))
                .unwrap()
                .accepted_frames,
            0
        );
        assert_eq!(stream.pull_interleaved(&mut []).unwrap().end, None);
        let output = drain(&mut stream);
        assert_eq!(
            output.len() / 2,
            frames + 48000 + usize::from(frames % 2048 == 0)
        );
        let mut graph =
            LocalStereoSubgraph::new(Format::new(48000.0, 2, 2048).unwrap(), plan()).unwrap();
        assert_eq!(bits(&output), bits(&reference(&mut graph, &input)));
    }
}
#[test]
fn insufficient_tail_capacity_rejects_before_eof_and_can_retry_after_reading() {
    let mut stream = LocalStereoStream::new(48000, 2, plan(), 52096).unwrap();
    let initial = stream.push_interleaved(&samples(4096), Some(1000)).unwrap();
    assert!(!initial.input_end);
    assert!(stream.push_interleaved(&[], None).is_err());
    let mut buffered = vec![0.0; 2 * initial.output_ready as usize];
    let read = stream.pull_interleaved(&mut buffered).unwrap();
    assert_eq!(read.frames, initial.output_ready);
    assert_eq!(read.end, Some(false));
    let end = stream.push_interleaved(&[], None).unwrap();
    assert!(end.input_end);
    assert_eq!(end.clock_ms, Some(1084));
    assert!(!drain(&mut stream).is_empty());
    assert!(LocalStereoStream::new(96000, 2, plan(), 65536).is_err());
    assert!(LocalStereoStream::new(48000, 1, plan(), 131072).is_err());
    assert!(LocalStereoStream::new(48000, 0, plan(), 131072).is_err());
}
#[test]
fn seek_reset_discards_queued_pcm_but_preserves_parent_gain_history() {
    let format = Format::new(48000.0, 2, 2048).unwrap();
    let input = samples(5000);
    let mut stream = LocalStereoStream::new(48000, 2, plan(), 131072).unwrap();
    let mut graph = LocalStereoSubgraph::new(format, plan()).unwrap();
    stream.push_interleaved(&input[..4096], Some(1000)).unwrap();
    let mut block = PlanarBlock::new(format);
    block.load_interleaved(&input[..4096]).unwrap();
    graph.process(&mut block, 2048, false).unwrap();
    stream.reset().unwrap();
    graph.reset().unwrap();
    let empty = stream.pull_interleaved(&mut [0.0; 2]).unwrap();
    assert_eq!(empty.frames, 0);
    assert_eq!(empty.end, Some(false));
    stream.push_interleaved(&input, Some(2000)).unwrap();
    stream.push_interleaved(&[], None).unwrap();
    let output = drain(&mut stream);
    assert_eq!(bits(&output), bits(&reference(&mut graph, &input)));
    let mut fresh = LocalStereoSubgraph::new(format, plan()).unwrap();
    assert_ne!(
        bits(&output),
        bits(&reference(&mut fresh, &input)),
        "recreation would lose the retained SE gain ramp"
    );
}
#[test]
fn local_plan_rejects_other_graphs_and_does_not_grant_remote_materials() {
    let base = json!({"eq":{"on":false,"eqs":vec![0;10]},"bt":{"on":false,"bass":0,"treble":0},"rvb":{"on":false},"se":{"on":true,"presence":3,"stereoizer":5,"sshaper":true,"ambience":1}});
    assert!(LocalStereoMaterialPlan::from_equalizer_json(&base).is_ok());
    for (path, value) in [
        ("eq", json!({"on":true,"eqs":vec![0;10]})),
        ("rvb", json!({"on":true})),
        (
            "se",
            json!({"on":true,"presence":0,"stereoizer":0,"sshaper":true,"ambience":1}),
        ),
        (
            "se",
            json!({"on":true,"presence":3,"stereoizer":5,"sshaper":true,"ambience":2}),
        ),
    ] {
        let mut bad = base.clone();
        bad[path] = value;
        assert!(LocalStereoMaterialPlan::from_equalizer_json(&bad).is_err());
    }
    let mut remote = base;
    remote["aev2"] = json!({"peq":{"on":true}});
    assert!(LocalStereoMaterialPlan::from_equalizer_json(&remote).is_err());
}

#[test]
fn real_cli_subgraph_and_fifo_expose_same_pcm_and_source_clock() {
    let material = json!({"eq":{"on":false,"eqs":vec![0;10]},"bt":{"on":false,"bass":0,"treble":0},"rvb":{"on":false},"se":{"on":true,"presence":3,"stereoizer":5,"sshaper":true,"ambience":1}});
    let input = samples(128);
    let mut blocks = vec![json!({"samples":input,"end":true})];
    blocks.extend((0..25).map(|_| json!({"samples":[],"end":true})));
    let rendered = wire::render(
        &serde_json::to_vec(
            &json!({"sampleRate":48000,"maxFrames":2048,"material":material,"blocks":blocks}),
        )
        .unwrap(),
    )
    .unwrap();
    let streamed=wire::stream(&serde_json::to_vec(&json!({"sampleRate":48000,"decodedChannels":2,"fifoCapacity":131072,"material":material,"actions":[{"kind":"push","samples":input,"timestampMs":1000},{"kind":"push","samples":[]},{"kind":"read","frames":65536}]})).unwrap()).unwrap();
    assert_eq!(rendered["fullSdkGraph"], false);
    assert_eq!(streamed["completeGraph"], false);
    let rendered = rendered["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| b["samples"].as_array().unwrap())
        .map(|v| (v.as_f64().unwrap() as f32).to_bits())
        .collect::<Vec<_>>();
    let read = &streamed["actions"][2];
    let actual = read["samples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v.as_f64().unwrap() as f32).to_bits())
        .collect::<Vec<_>>();
    assert_eq!(actual, rendered);
    assert_eq!(actual.len() / 2, 48128);
    assert_eq!(read["status"]["end"], true);
    assert_eq!(streamed["actions"][1]["status"]["clockMs"], 1002);
}
