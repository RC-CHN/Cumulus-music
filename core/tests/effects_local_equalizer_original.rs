//! Synthetic PCM comparison to the original SDK for the bounded local EQ lane.
#![cfg(all(
    any(target_os = "linux", target_os = "windows"),
    target_arch = "x86_64"
))]

use cumulus_core::effects::{
    dsp::{Format, PlanarBlock},
    stereo::{LocalEqualizerMaterialPlan, LocalEqualizerStereoMaterial},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Read;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Corpus {
    schema: String,
    original_outer_create_init_and_controls_executed: bool,
    original_inner_graph_executed: bool,
    original_outer_fifo_executed: bool,
    cloud_cef_json_executed: bool,
    native_windows: bool,
    dsp_replaced: bool,
    input_blocks: Vec<Vec<u32>>,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    initial_material: Value,
    input_frames: usize,
    output_frames: usize,
    blocks: Vec<Block>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Block {
    source_first_frame: usize,
    input_frames: usize,
    input_end: bool,
    input_block: usize,
    action_before: Option<Action>,
    output_frames: usize,
    output_end: bool,
    output_sha256: String,
    output_prefix_interleaved_bits: Option<Vec<u32>>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Action {
    FullApply { material: Value },
    MasterOff,
    SeekReset,
}

struct CallerFtz(u32);
impl CallerFtz {
    fn cleared() -> Self {
        let mut saved = 0;
        // SAFETY: two live u32 buffers. Change only the caller FTZ bit, the
        // mode used by the original oracle for wrapper blends and B0.
        unsafe {
            std::arch::asm!("stmxcsr [{}]", in(reg) &mut saved, options(nostack, preserves_flags));
            let value = saved & !0x8000;
            std::arch::asm!("ldmxcsr [{}]", in(reg) &value, options(nostack, preserves_flags));
        }
        Self(saved)
    }
}
impl Drop for CallerFtz {
    fn drop(&mut self) {
        // SAFETY: restore exactly the caller's saved valid value.
        unsafe {
            std::arch::asm!("ldmxcsr [{}]", in(reg) &self.0, options(nostack, preserves_flags));
        }
    }
}

#[test]
fn original_local_equalizer_pcm_including_hot_apply_reset_and_master_edges() {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("fixtures/effects-local-equalizer-full-official.json.gz")[..],
    )
    .take(2 * 1024 * 1024)
    .read_to_end(&mut bytes)
    .unwrap();
    let corpus: Corpus = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(corpus.schema, "cumulus-local-equalizer-full-official-v1");
    assert!(corpus.original_outer_create_init_and_controls_executed);
    assert!(corpus.original_inner_graph_executed);
    assert!(!corpus.original_outer_fifo_executed);
    assert!(!corpus.cloud_cef_json_executed);
    assert!(!corpus.native_windows);
    assert!(!corpus.dsp_replaced);
    let expected = [
        ("geq_only_hot_and_seek_reset", 5, 8199, 7980, [1, 0, 1]),
        ("bt_only_hot_and_seek_reset", 5, 8199, 7980, [1, 0, 1]),
        (
            "geq_bt_joint_component_off_edges",
            6,
            10247,
            10247,
            [3, 0, 0],
        ),
        ("geq_bt_se1_joint_hot", 25, 6151, 50251, [1, 0, 0]),
        (
            "geq_bt_se1_master_off_through_eof",
            5,
            8199,
            8199,
            [0, 1, 0],
        ),
        ("geq_bt_se1_master_off_reopen", 28, 12295, 56395, [1, 1, 0]),
        ("geq_only_master_off", 4, 6151, 6151, [0, 1, 0]),
        ("bt_only_master_off", 4, 6151, 6151, [0, 1, 0]),
        ("geq_bt_master_off", 4, 6151, 6151, [0, 1, 0]),
        ("geq_se1_master_off", 4, 6151, 6151, [0, 1, 0]),
        ("bt_se1_master_off", 4, 6151, 6151, [0, 1, 0]),
        ("all_components_off_master_off", 4, 6151, 6151, [0, 1, 0]),
    ];
    assert_eq!(corpus.cases.len(), expected.len());
    assert_eq!(corpus.input_blocks.len(), 14);
    let format = Format::new(44100.0, 2, 2048).unwrap();
    let mut checked_prefixes = 0;
    for (case, (name, count, input, output, actions)) in corpus.cases.into_iter().zip(expected) {
        let _caller = CallerFtz::cleared();
        assert_eq!(case.name, name);
        assert_eq!(case.blocks.len(), count, "{name}");
        assert_eq!((case.input_frames, case.output_frames), (input, output));
        let plan = LocalEqualizerMaterialPlan::from_equalizer_json(&case.initial_material).unwrap();
        let mut graph = LocalEqualizerStereoMaterial::new(format, plan).unwrap();
        assert_eq!(graph.lookahead_frames(), 219);
        let mut pcm = PlanarBlock::new(format);
        let (mut consumed, mut emitted) = (0, 0);
        let mut observed_actions = [0; 3];
        for (index, block) in case.blocks.into_iter().enumerate() {
            let label = format!("{name} block {index}");
            assert_eq!(block.source_first_frame, consumed, "{label}");
            let input_bits = &corpus.input_blocks[block.input_block];
            assert_eq!(input_bits.len(), block.input_frames * 2);
            pcm.samples_mut().fill(0.0);
            for (frame, pair) in input_bits.chunks_exact(2).enumerate() {
                pcm.samples_mut()[frame] = f32::from_bits(pair[0]);
                pcm.samples_mut()[format.capacity() + frame] = f32::from_bits(pair[1]);
            }
            match block.action_before {
                Some(Action::FullApply { material }) => {
                    graph
                        .apply_material(
                            LocalEqualizerMaterialPlan::from_equalizer_json(&material).unwrap(),
                        )
                        .unwrap();
                    observed_actions[0] += 1;
                }
                Some(Action::MasterOff) => {
                    graph.disable_master().unwrap();
                    observed_actions[1] += 1;
                }
                Some(Action::SeekReset) => {
                    graph.reset().unwrap();
                    observed_actions[2] += 1;
                }
                None => (),
            }
            let flow = graph
                .process(&mut pcm, block.input_frames, block.input_end)
                .unwrap();
            assert_eq!(
                (flow.valid_frames, flow.end),
                (block.output_frames, block.output_end),
                "{label} frame/EOF"
            );
            assert_eq!(flow.end, index + 1 == count, "{label}");
            let bits = (0..flow.valid_frames)
                .flat_map(|i| {
                    [
                        pcm.samples()[i].to_bits(),
                        pcm.samples()[format.capacity() + i].to_bits(),
                    ]
                })
                .collect::<Vec<_>>();
            let mut digest = Sha256::new();
            for &word in &bits {
                digest.update(word.to_le_bytes());
            }
            assert_eq!(
                hex::encode(digest.finalize()),
                block.output_sha256,
                "{label} final PCM"
            );
            if let Some(prefix) = block.output_prefix_interleaved_bits {
                assert!(!prefix.is_empty() && prefix.len().is_multiple_of(2));
                assert_eq!(
                    &bits[..prefix.len()],
                    prefix.as_slice(),
                    "{label} selected PCM"
                );
                checked_prefixes += 1;
            }
            consumed += block.input_frames;
            emitted += flow.valid_frames;
        }
        assert_eq!(observed_actions, actions, "{name} original operations");
        assert_eq!((consumed, emitted), (input, output), "{name} final drain");
        pcm.samples_mut().fill(0.0);
        let eof = graph.process(&mut pcm, 0, true).unwrap();
        assert_eq!(
            (eof.valid_frames, eof.end),
            (0, true),
            "{name} repeated EOF"
        );
    }
    assert_eq!(checked_prefixes, 43);
}

#[test]
fn fixed_local_equalizer_rejects_other_schemas_and_geometry() {
    let fixture = json!({
        "eq":{"on":true,"eqs":[-1.5,-1,0,1.79,4,3.79,1.79,0.5,-1,1]},
        "bt":{"on":true,"bass":4,"treble":-3},
        "se":{"on":true,"presence":3,"stereoizer":5,"sshaper":true,"ambience":1},
        "rvb":{"on":false,"er":{"on":true,"pattern":15},"rvb":{"density":80}},
    });
    let plan = LocalEqualizerMaterialPlan::from_equalizer_json(&fixture).unwrap();
    for (rate, channels, capacity) in [(48000.0, 2, 2048), (44100.0, 1, 2048), (44100.0, 2, 512)] {
        assert!(
            LocalEqualizerStereoMaterial::new(Format::new(rate, channels, capacity).unwrap(), plan)
                .is_err()
        );
    }
    for (path, value) in [
        ("/rvb/on", json!(true)),
        ("/eq/on", json!(false)),
        ("/eq/eqs/0", json!(12.01)),
        ("/eq/eqs/0", json!("1.79")),
        ("/bt/bass", json!(2.5)),
        ("/bt/treble", json!(11)),
        ("/se/stereoizer", json!(-1)),
        ("/se/ambience", json!(0)),
        ("/se/sshaper", json!(false)),
    ] {
        let mut invalid = fixture.clone();
        *invalid.pointer_mut(path).unwrap() = value;
        assert!(
            LocalEqualizerMaterialPlan::from_equalizer_json(&invalid).is_err(),
            "{path}"
        );
    }
    assert!(LocalEqualizerMaterialPlan::from_equalizer_json(&json!({"aev2": fixture})).is_err());
}
