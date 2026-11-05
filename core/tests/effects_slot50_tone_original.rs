//! Synthetic PCM hashes from the original x64 slot 50, without the SDK image.
#![cfg(all(
    any(target_os = "linux", target_os = "windows"),
    target_arch = "x86_64"
))]

use cumulus_core::effects::dsp::{Format, PlanarBlock, Slot50Material, Slot50Tone};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Oracle {
    schema: String,
    dll_sha256: String,
    node_rva: String,
    capacity: u32,
    original_text_unmodified: bool,
    full_original_sdk_graph: bool,
    original_outer_fifo: bool,
    real_windows_execution: bool,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    outer_format_enum: u32,
    rate: u32,
    channels: u32,
    blocks: Vec<Block>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Block {
    action: Action,
    frames: usize,
    end: bool,
    input_sha256: String,
    output_sha256: String,
    before: [u8; 2],
    after: [u8; 2],
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    FreshDeviceReset,
    MaterialEnable,
    SameControls,
    MaterialHot,
    MasterDry,
    MasterStaysDry,
    FullReapply,
    MasterDryThenReset,
    FullReapplyThenReset,
    MaterialDisable,
    OffStaysDry,
}

struct Mxcsr(u32);

impl Mxcsr {
    fn original_fixture_mode() -> Self {
        let mut saved = 0;
        // SAFETY: valid four-byte MXCSR destination and the original x86_64
        // default value, with no reserved bits set. Restored on this thread.
        unsafe {
            std::arch::asm!("stmxcsr [{}]", in(reg) &mut saved, options(nostack, preserves_flags));
            std::arch::asm!("ldmxcsr [{}]", in(reg) &0x1f80u32, options(nostack, preserves_flags));
        }
        Self(saved)
    }
}

impl Drop for Mxcsr {
    fn drop(&mut self) {
        // SAFETY: restored value was read from the same CPU register.
        unsafe {
            std::arch::asm!("ldmxcsr [{}]", in(reg) &self.0, options(nostack, preserves_flags));
        }
    }
}

fn fixture() -> Oracle {
    serde_json::from_str(include_str!("fixtures/effects-slot50-tone-official.json")).unwrap()
}

fn sha(block: &PlanarBlock) -> String {
    let mut digest = Sha256::new();
    for sample in block.samples() {
        digest.update(sample.to_bits().to_le_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn synthetic_input(block: &mut PlanarBlock, index: usize, frames: usize) {
    for (channel, plane) in block.samples_mut().chunks_exact_mut(2048).enumerate() {
        plane.fill(0.0);
        for (frame, sample) in plane[..frames].iter_mut().enumerate() {
            let value = ((frame * 13 + index * 7 + channel * 19) % 113) as i32 - 56;
            *sample = value as f32 / 1024.0;
        }
        if frames != 0 {
            plane[0] = if channel == 0 { 0.5 } else { -0.375 };
        }
    }
}

#[test]
fn official_slot50_pcm_matches_full_capacity_including_short_transition_tails() {
    let _fp = Mxcsr::original_fixture_mode();
    let oracle = fixture();
    assert_eq!(oracle.schema, "cumulus-slot50-tone-official-v1");
    assert_eq!(
        oracle.dll_sha256,
        "c59105e1072d7d7dbd2d2844ac9c170de7e7b41eb463dc56a11c9b37bafac6be"
    );
    assert_eq!(oracle.node_rva, "a2970");
    assert!(oracle.original_text_unmodified);
    assert!(!oracle.full_original_sdk_graph && !oracle.original_outer_fifo);
    assert!(!oracle.real_windows_execution);
    assert_eq!(oracle.cases.len(), 8);

    let mut seen = std::collections::BTreeSet::new();
    for case in oracle.cases {
        assert!([3, 4].contains(&case.outer_format_enum));
        assert!(seen.insert((case.outer_format_enum, case.rate, case.channels)));
        let format = Format::new(case.rate as f32, case.channels, oracle.capacity).unwrap();
        let mut tone = Slot50Tone::new(format).unwrap();
        let mut pcm = PlanarBlock::new(tone.format());
        let initial = Slot50Material::new(4, -3).unwrap();
        let hot = Slot50Material::new(-2, 6).unwrap();
        assert_eq!(case.blocks.len(), 11);
        for (index, block) in case.blocks.iter().enumerate() {
            match block.action {
                // Constructor already performs this explicit device reset.
                Action::FreshDeviceReset
                | Action::SameControls
                | Action::MasterStaysDry
                | Action::OffStaysDry => {}
                Action::MaterialEnable | Action::FullReapply => {
                    tone.apply_material(initial).unwrap()
                }
                Action::MaterialHot => tone.apply_material(hot).unwrap(),
                Action::MasterDry => tone.master_bypass().unwrap(),
                Action::MasterDryThenReset => {
                    tone.master_bypass().unwrap();
                    tone.reset().unwrap();
                }
                Action::FullReapplyThenReset => {
                    tone.apply_material(initial).unwrap();
                    tone.reset().unwrap();
                }
                Action::MaterialDisable => tone.apply_material(Slot50Material::default()).unwrap(),
            }
            assert_eq!(block.end, index >= 9);
            assert_eq!(block.after, [block.before[0]; 2]);
            synthetic_input(&mut pcm, index, block.frames);
            assert_eq!(sha(&pcm), block.input_sha256, "fixture input block {index}");
            tone.process(&mut pcm, block.frames).unwrap();
            assert_eq!(
                sha(&pcm),
                block.output_sha256,
                "slot50 enum={} rate={} channels={} block={index} frames={}",
                case.outer_format_enum,
                case.rate,
                case.channels,
                block.frames
            );
        }
    }
}

#[test]
fn rejects_unproven_formats_and_does_not_consume_a_pending_edge_on_invalid_pcm() {
    let _fp = Mxcsr::original_fixture_mode();
    for format in [(44_101.0, 1, 2048), (48_000.0, 3, 2048), (48_000.0, 2, 512)] {
        assert!(Slot50Tone::new(Format::new(format.0, format.1, format.2).unwrap()).is_err());
    }
    assert!(Slot50Material::new(-11, 0).is_err());
    assert!(Slot50Material::new(0, 11).is_err());
    let oracle = fixture();
    let first_wet = &oracle.cases[0].blocks[1];
    let mut tone = Slot50Tone::new(Format::new(44_100.0, 1, 2048).unwrap()).unwrap();
    tone.apply_material(Slot50Material::new(4, -3).unwrap())
        .unwrap();
    let mut wrong = PlanarBlock::new(Format::new(44_100.0, 2, 2048).unwrap());
    assert!(tone.process(&mut wrong, 2048).is_err());
    let mut pcm = PlanarBlock::new(tone.format());
    synthetic_input(&mut pcm, 1, first_wet.frames);
    assert!(tone.process(&mut pcm, 2049).is_err());
    assert_eq!(sha(&pcm), first_wet.input_sha256);
    pcm.samples_mut()[0] = f32::NAN;
    assert!(tone.process(&mut pcm, first_wet.frames).is_err());
    pcm.samples_mut()[0] = 0.5;
    tone.process(&mut pcm, first_wet.frames).unwrap();
    assert_eq!(sha(&pcm), first_wet.output_sha256);
}
