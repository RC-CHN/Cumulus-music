use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io::Read, sync::OnceLock};

struct Ftz(u32);
impl Ftz {
    fn off() -> Self {
        let mut old = 0u32;
        // SAFETY: valid four-byte MXCSR destination on the gated x86_64 test.
        unsafe { std::arch::asm!("stmxcsr [{}]",in(reg)&mut old,options(nostack,preserves_flags)) };
        let value = old & !0x8000;
        // SAFETY: change only the caller's valid FTZ bit to match the corpus.
        unsafe { std::arch::asm!("ldmxcsr [{}]",in(reg)&value,options(nostack,preserves_flags)) };
        Self(old)
    }
}
impl Drop for Ftz {
    fn drop(&mut self) {
        // SAFETY: restore exactly the thread's previously saved MXCSR value.
        unsafe { std::arch::asm!("ldmxcsr [{}]",in(reg)&self.0,options(nostack,preserves_flags)) };
    }
}

fn plan(enabled: bool, presence: i32, stereoizer: i32) -> LocalStereoMaterialPlan {
    LocalStereoMaterialPlan::from_equalizer_json(&json!({"eq":{"on":false,"eqs":vec![0;10]},"bt":{"on":false,"bass":0,"treble":0},"rvb":{"on":false},"se":{"on":enabled,"presence":if enabled {presence}else{0},"stereoizer":if enabled{stereoizer}else{0},"sshaper":true,"ambience":1}})).unwrap()
}
fn decode(bytes: &[u8]) -> Value {
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .take(16 * 1024 * 1024)
        .read_to_end(&mut raw)
        .unwrap();
    serde_json::from_slice(&raw).unwrap()
}
fn full() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        decode(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/effects-local-stereo-full-official.json.gz"
        )))
    })
}
fn step(graph: &mut SourceGraph, row: &Value) {
    step_using(graph, row, row["inputInterleavedBits"].as_array().unwrap());
}
fn step_using(graph: &mut SourceGraph, row: &Value, input: &[Value]) {
    let mut pcm = PlanarBlock::new(graph.format());
    let values = input
        .iter()
        .map(|v| f32::from_bits(v.as_u64().unwrap() as u32))
        .collect::<Vec<_>>();
    let count = pcm.load_interleaved(&values).unwrap();
    assert_eq!(count, row["inputFrames"].as_u64().unwrap() as usize);
    let flow = graph
        .process(&mut pcm, count, row["inputEnd"].as_bool().unwrap())
        .unwrap();
    assert_eq!(
        flow.valid_frames,
        row["outputFrames"].as_u64().unwrap() as usize
    );
    assert_eq!(flow.end, row["outputEnd"].as_bool().unwrap());
    let mut bytes = Vec::new();
    for i in 0..flow.valid_frames {
        bytes.extend_from_slice(&pcm.samples()[i].to_le_bytes());
        bytes.extend_from_slice(&pcm.samples()[graph.format().capacity() + i].to_le_bytes());
    }
    assert_eq!(
        hex::encode(Sha256::digest(bytes)),
        row["outputSha256"].as_str().unwrap()
    );
}

#[test]
fn unused_decoder_preparation_preserves_the_original_fresh_off_edges() {
    let _ftz = Ftz::off();
    let case = full()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "se1_enabled" && v["rate"] == 44100 && v["capacity"] == 2048)
        .unwrap();
    let mut graph =
        SourceGraph::new(Format::new(44100.0, 2, 2048).unwrap(), plan(true, 3, 5)).unwrap();
    graph.prepare(0).unwrap();
    graph.configure(plan(true, 3, 5)).unwrap();
    graph.prepare(0).unwrap();
    assert!(
        !graph.driven,
        "decoder setup/cancel before PCM cannot suppress fresh off transitions"
    );
    for row in case["blocks"].as_array().unwrap() {
        step(&mut graph, row);
    }
    assert!(graph.driven);
}

#[test]
fn first_nonzero_logical_seek_uses_reset_without_consuming_a_fake_pcm_frame() {
    let _ftz = Ftz::off();
    let format = Format::new(44100.0, 2, 2048).unwrap();
    let mut graph = SourceGraph::new(format, plan(true, 3, 5)).unwrap();
    let mut expected = LocalFixedStereoMaterial::new(format, plan(true, 3, 5)).unwrap();
    expected.reset().unwrap();
    graph.prepare(123).unwrap();
    assert!(graph.driven);
    let row = &full()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "se1_enabled" && v["rate"] == 44100 && v["capacity"] == 2048)
        .unwrap()["blocks"][0];
    let input = row["inputInterleavedBits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| f32::from_bits(v.as_u64().unwrap() as u32))
        .collect::<Vec<_>>();
    let mut actual_pcm = PlanarBlock::new(format);
    let mut expected_pcm = PlanarBlock::new(format);
    let count = actual_pcm.load_interleaved(&input).unwrap();
    expected_pcm.load_interleaved(&input).unwrap();
    let actual = graph.process(&mut actual_pcm, count, false).unwrap();
    let expected_flow = expected.process(&mut expected_pcm, count, false).unwrap();
    assert_eq!(actual, expected_flow);
    assert_eq!(actual_pcm.samples(), expected_pcm.samples());
}

#[test]
fn decoded_packet_fifo_delivers_the_frozen_complete_fixed_pcm_to_its_reader() {
    use crate::playback::engine::{
        Cancellation,
        limiter_segment::{DecodedPacket, DecoderPayload},
        stereo_pcm::channel,
    };
    use std::{collections::VecDeque, thread, time::Instant};

    let case = full()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "se1_enabled" && v["rate"] == 44100 && v["capacity"] == 2048)
        .unwrap();
    let total = case["inputFrames"].as_u64().unwrap();
    let mut packets = VecDeque::new();
    let mut cursor = 0;
    for row in case["blocks"].as_array().unwrap() {
        let input = row["inputInterleavedBits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| f32::from_bits(v.as_u64().unwrap() as u32))
            .collect::<Vec<_>>();
        for samples in input.chunks(1024 * 2) {
            packets.push_back(DecodedPacket {
                generation: super::super::Generation(1),
                first_source_frame: cursor,
                payload: DecoderPayload::Samples(samples.to_vec()),
            });
            cursor += (samples.len() / 2) as u64;
        }
    }
    assert_eq!(cursor, total);
    packets.push_back(DecodedPacket {
        generation: super::super::Generation(1),
        first_source_frame: total,
        payload: DecoderPayload::End,
    });
    // Bound a failed fixture too: it must not leave a blocking read waiting
    // forever if a future FIFO change omits a frame or the terminal EOF.
    let deadline = Instant::now() + std::time::Duration::from_secs(8);
    let (mut reader, control, shared) = channel(
        44100,
        0,
        total,
        Cancellation::new(move || Instant::now() < deadline),
        None,
    )
    .unwrap();
    control.publish_sink().unwrap();
    control.bind_entry(77).unwrap();
    control.observe(77, Some(0.0), false, false).unwrap();
    let worker = thread::spawn(move || {
        let _ftz = Ftz::off();
        let mut graph =
            SourceGraph::new(Format::new(44100.0, 2, 2048).unwrap(), plan(true, 3, 5)).unwrap();
        graph.prepare(0).unwrap();
        let result = super::super::pump(&mut graph, 0, total, &shared, || Ok(packets.pop_front()));
        if result.is_err() {
            shared.fail();
        }
        result
    });
    let mut delivered = 0;
    for row in case["blocks"].as_array().unwrap() {
        let frames = row["outputFrames"].as_u64().unwrap();
        let mut bytes = vec![0; frames as usize * 8];
        reader.read_exact(&mut bytes).unwrap();
        assert_eq!(
            hex::encode(Sha256::digest(bytes)),
            row["outputSha256"].as_str().unwrap()
        );
        delivered += frames;
        control
            .observe(77, Some(delivered as f64 / 44100.0), false, false)
            .unwrap();
    }
    assert_eq!(reader.read(&mut [0; 1]).unwrap(), 0);
    worker.join().unwrap().unwrap();
    let state = control.snapshot().unwrap();
    assert!(state.graph_end);
    assert_eq!(state.decoded_source_frame, total);
    assert_eq!(state.delivered_frames, total + 44100);
    assert_eq!(state.native_source_frame, total);
    drop(reader);
    control.retire_sink();
}

#[test]
fn reused_source_applies_the_actual_original_seek_reset_without_reconstructing_it() {
    let _ftz = Ftz::off();
    let case = full()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "seek_reset" && v["rate"] == 44100 && v["capacity"] == 2048)
        .unwrap();
    let mut graph =
        SourceGraph::new(Format::new(44100.0, 2, 2048).unwrap(), plan(true, 3, 5)).unwrap();
    graph.prepare(0).unwrap();
    for row in case["blocks"].as_array().unwrap() {
        if !row["actionBefore"].is_null() {
            graph
                .prepare(row["sourceFirstFrame"].as_u64().unwrap())
                .unwrap();
        }
        step(&mut graph, row);
    }
}

#[test]
fn fixed_source_owner_preserves_bank_and_hot_controls_across_off_and_pending_reset() {
    let _ftz = Ftz::off();
    let data = decode(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/effects-local-stereo-slider-official.json.gz"
    )));
    let case = &data["cases"][0];
    assert_eq!(case["rate"], 44100);
    let mut graph =
        SourceGraph::new(Format::new(44100.0, 2, 2048).unwrap(), plan(false, 0, 0)).unwrap();
    graph.prepare(0).unwrap();
    let (mut enabled, mut presence, mut stereoizer) = (false, 0, 0);
    for row in case["blocks"].as_array().unwrap() {
        for action in row["actionBefore"].as_array().into_iter().flatten() {
            match action["kind"].as_str().unwrap() {
                "original_se_controls" => {
                    presence = action["presence"].as_f64().unwrap() as i32;
                    stereoizer = action["stereoizer"].as_f64().unwrap() as i32;
                    if enabled {
                        graph.configure(plan(true, presence, stereoizer)).unwrap();
                    }
                }
                "original_se_enable" => {
                    enabled = action["enabled"].as_bool().unwrap();
                    graph
                        .configure(plan(enabled, presence, stereoizer))
                        .unwrap();
                }
                "original_graph_seek_reset" => graph
                    .prepare(action["newSyntheticSourceFrame"].as_u64().unwrap())
                    .unwrap(),
                _ => panic!("unexpected original action"),
            }
        }
        step(&mut graph, row);
    }
}

#[test]
fn complete_eof_source_is_reused_for_another_original_tail_after_logical_seek() {
    let _ftz = Ftz::off();
    let data = decode(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/effects-local-stereo-after-eof-reset-official.json.gz"
    )));
    let mut graph =
        SourceGraph::new(Format::new(48000.0, 2, 2048).unwrap(), plan(true, 3, 5)).unwrap();
    graph.prepare(0).unwrap();
    for phase in data["phases"].as_array().unwrap() {
        for row in phase["blocks"].as_array().unwrap() {
            if !row["actionBefore"].is_null() {
                graph
                    .prepare(row["sourceFirstFrame"].as_u64().unwrap())
                    .unwrap();
            }
            step(&mut graph, row);
        }
    }
}

#[test]
fn native_graph_preserves_original_initial_edges_and_same_owner_control_reset() {
    let _ftz = Ftz::off();
    let source = decode(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/effects-local-equalizer-full-official.json.gz"
    )));
    let inputs = source["inputBlocks"].as_array().unwrap();
    let cases = source["cases"].as_array().unwrap();
    for name in [
        "geq_only_hot_and_seek_reset",
        "geq_bt_se1_master_off_reopen",
    ] {
        let case = cases.iter().find(|case| case["name"] == name).unwrap();
        let plan =
            LocalEqualizerMaterialPlan::from_equalizer_json(&case["initialMaterial"]).unwrap();
        let mut graph =
            SourceGraph::new_local(Format::new(44100.0, 2, 2048).unwrap(), plan).unwrap();
        // Constructor already reset then applied. Decoder preparation alone
        // must not settle the new dry-to-wet wrappers or invoke a fake block.
        graph.prepare(0).unwrap();
        graph.prepare(0).unwrap();
        assert!(!graph.driven);
        for row in case["blocks"].as_array().unwrap() {
            let action = &row["actionBefore"];
            match action["kind"].as_str() {
                None => assert!(action.is_null()),
                Some("full_apply") => graph
                    .apply(StereoGraphControl::ApplyLocal(
                        LocalEqualizerMaterialPlan::from_equalizer_json(&action["material"])
                            .unwrap(),
                    ))
                    .unwrap(),
                Some("master_off") => graph.apply(StereoGraphControl::DisableLocalMaster).unwrap(),
                Some("seek_reset") => graph
                    .prepare(row["sourceFirstFrame"].as_u64().unwrap())
                    .unwrap(),
                Some(other) => panic!("unexpected original action: {other}"),
            }
            step_using(
                &mut graph,
                row,
                inputs[row["inputBlock"].as_u64().unwrap() as usize]
                    .as_array()
                    .unwrap(),
            );
        }
    }
}
