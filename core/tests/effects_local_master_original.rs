//! Original outer controls with inner 1AA50 PCM, in the fixed local scope.
//! The inputs are synthetic and the fixture contains no original PE material.
#![cfg(all(
    any(target_os = "linux", target_os = "windows"),
    target_arch = "x86_64"
))]

use cumulus_core::effects::{
    dsp::{Format, PlanarBlock},
    stereo::{LocalFixedStereoMaster, LocalStereoMaterialPlan},
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::Read;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Corpus {
    schema: String,
    original_outer_create_init_executed: bool,
    original_outer_master_and_material_controls_executed: bool,
    original_outer_pcm_fifo_executed: bool,
    cloud_cef_json_executed: bool,
    real_windows_execution: bool,
    dsp_replaced: bool,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
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
    input_interleaved_bits: Vec<u32>,
    action_before: Option<Action>,
    output_frames: usize,
    output_end: bool,
    output_sha256: String,
    output_prefix_interleaved_bits: Option<Vec<u32>>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Action {
    MasterOff,
    FullMaterialReopen { presence: i32, stereoizer: i32 },
}
fn plan(presence: i32, stereoizer: i32) -> LocalStereoMaterialPlan {
    LocalStereoMaterialPlan::from_equalizer_json(&json!({
        "eq":{"on":false,"eqs":vec![0;10]}, "bt":{"on":false,"bass":0,"treble":0},
        "rvb":{"on":false}, "se":{"on":presence != 0 || stereoizer != 0,
        "presence":presence, "stereoizer":stereoizer, "sshaper":true, "ambience":1},
    }))
    .unwrap()
}
struct CallerFtz(u32);
impl CallerFtz {
    fn cleared() -> Self {
        let mut saved = 0;
        // SAFETY: both operands point to live four-byte MXCSR storage. Clear
        // only FTZ for the original harness's caller floating-point profile.
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
        // SAFETY: restore the exact valid value previously read from MXCSR.
        unsafe {
            std::arch::asm!("ldmxcsr [{}]", in(reg) &self.0, options(nostack, preserves_flags));
        }
    }
}

#[test]
fn original_fixed_local_master_keeps_b0_lookahead_and_restores_material_tail() {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("fixtures/effects-local-stereo-master-official.json.gz")[..],
    )
    .take(2 * 1024 * 1024)
    .read_to_end(&mut bytes)
    .unwrap();
    let corpus: Corpus = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(corpus.schema, "cumulus-original-fixed-master-controls-v1");
    assert!(corpus.original_outer_create_init_executed);
    assert!(corpus.original_outer_master_and_material_controls_executed);
    assert!(!corpus.original_outer_pcm_fifo_executed);
    assert!(!corpus.cloud_cef_json_executed);
    assert!(!corpus.real_windows_execution);
    assert!(!corpus.dsp_replaced);
    let expected = [
        ("master_off_through_eof", 5, 8199, 8199, None),
        (
            "master_off_then_full_material_reopen",
            28,
            12295,
            56395,
            Some((3, 5)),
        ),
        (
            "master_reopen_changed_controls_with_retained_b0_peaks",
            28,
            12295,
            56395,
            Some((6, 8)),
        ),
    ];
    assert_eq!(corpus.cases.len(), expected.len());
    let format = Format::new(44100.0, 2, 2048).unwrap();
    let mut checked_prefixes = 0;
    for (case, (name, count, input, output, expected_reopen)) in
        corpus.cases.into_iter().zip(expected)
    {
        assert_eq!(case.name, name);
        assert_eq!(case.blocks.len(), count, "{name}");
        assert_eq!((case.input_frames, case.output_frames), (input, output));
        let _caller = CallerFtz::cleared();
        let mut graph = LocalFixedStereoMaster::new(format, plan(3, 5)).unwrap();
        assert_eq!(graph.lookahead_frames(), 219);
        let mut pcm = PlanarBlock::new(format);
        let (mut consumed, mut emitted) = (0, 0);
        let (mut off, mut reopen) = (false, None);
        for (index, block) in case.blocks.into_iter().enumerate() {
            let label = format!("{name} block {index}");
            assert_eq!(block.source_first_frame, consumed, "{label}");
            assert_eq!(block.input_interleaved_bits.len(), block.input_frames * 2);
            pcm.samples_mut().fill(0.0);
            for (frame, pair) in block.input_interleaved_bits.chunks_exact(2).enumerate() {
                pcm.samples_mut()[frame] = f32::from_bits(pair[0]);
                pcm.samples_mut()[format.capacity() + frame] = f32::from_bits(pair[1]);
            }
            match block.action_before {
                Some(Action::MasterOff) => {
                    assert!(!off && consumed == 4096, "{label}");
                    graph.disable_master().unwrap();
                    off = true;
                }
                Some(Action::FullMaterialReopen {
                    presence,
                    stereoizer,
                }) => {
                    assert!(off && reopen.is_none() && consumed == 8192, "{label}");
                    // A component-off plan is not an admitted full master
                    // apply and must not affect the still-live off state.
                    assert!(graph.apply_material(plan(0, 0)).is_err());
                    graph.apply_material(plan(presence, stereoizer)).unwrap();
                    reopen = Some((presence, stereoizer));
                }
                None => (),
            }
            let flow = graph
                .process(&mut pcm, block.input_frames, block.input_end)
                .unwrap();
            assert_eq!(
                (flow.valid_frames, flow.end),
                (block.output_frames, block.output_end),
                "{label} frame and EOF contract",
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
            for &value in &bits {
                digest.update(value.to_le_bytes());
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
        assert!(off);
        assert_eq!(reopen, expected_reopen);
        assert_eq!(
            (consumed, emitted),
            (input, output),
            "{name} complete drain"
        );
        pcm.samples_mut().fill(0.0);
        let eof = graph.process(&mut pcm, 0, true).unwrap();
        assert_eq!(
            (eof.valid_frames, eof.end),
            (0, true),
            "{name} repeated EOF"
        );
    }
    assert_eq!(checked_prefixes, 13);
}

#[test]
fn fixed_master_rejects_unwitnessed_formats_and_component_only_off() {
    for (rate, channels, capacity) in [(48000.0, 2, 2048), (44100.0, 1, 2048), (44100.0, 2, 512)] {
        assert!(
            LocalFixedStereoMaster::new(Format::new(rate, channels, capacity).unwrap(), plan(3, 5))
                .is_err()
        );
    }
    assert!(
        LocalFixedStereoMaster::new(Format::new(44100.0, 2, 2048).unwrap(), plan(0, 0)).is_err()
    );
}
