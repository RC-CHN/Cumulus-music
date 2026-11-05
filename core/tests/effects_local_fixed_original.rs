//! Full original 1AA50 graph receipts for one explicit old-schema local scope.
//! No DLL or original parameter blobs are required in public CI.
#![cfg(all(
    any(target_os = "linux", target_os = "windows"),
    target_arch = "x86_64"
))]
use cumulus_core::effects::{
    dsp::{Format, PlanarBlock},
    stereo::{LocalFixedStereoMaterial, LocalStereoMaterialPlan},
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::Read;

#[derive(Deserialize)]
struct Corpus {
    schema: String,
    #[serde(rename = "fullOriginalSdkGraph")]
    full_sdk: bool,
    #[serde(rename = "realWindowsExecution")]
    real_windows: bool,
    #[serde(rename = "originalOuterFifo")]
    original_fifo: bool,
    #[serde(rename = "dspReplaced")]
    dsp_replaced: bool,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    rate: u32,
    capacity: u32,
    output_frames: u64,
    blocks: Vec<Block>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Block {
    input_frames: usize,
    input_end: bool,
    input_interleaved_bits: Vec<u32>,
    action_before: Option<Action>,
    output_frames: usize,
    output_end: bool,
    output_sha256: String,
    output_prefix_frames: Option<usize>,
    output_prefix_interleaved_bits: Option<Vec<u32>>,
    child_order: Vec<String>,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(tag = "kind")]
enum Action {
    #[serde(rename = "original_se_switch_off")]
    Disable,
    #[serde(
        rename = "original_graph_seek_reset",
        alias = "original_graph_seek_reset_after_complete_eof"
    )]
    Reset,
}
fn plan(enabled: bool) -> LocalStereoMaterialPlan {
    LocalStereoMaterialPlan::from_equalizer_json(&json!({
        "eq":{"on":false,"eqs":vec![0;10]},"bt":{"on":false,"bass":0,"treble":0},
        "rvb":{"on":false},"se":{"on":enabled,"presence":if enabled {3} else {0},
        "stereoizer":if enabled {5} else {0},"sshaper":true,"ambience":1}
    }))
    .unwrap()
}
fn mxcsr() -> u32 {
    let mut value = 0;
    // SAFETY: valid four-byte x86_64 MXCSR destination.
    unsafe { std::arch::asm!("stmxcsr [{}]",in(reg)&mut value,options(nostack,preserves_flags)) };
    value
}
struct Ftz(u32);
impl Ftz {
    fn off() -> Self {
        let old = mxcsr();
        let value = old & !0x8000;
        // SAFETY: only clear the valid caller FTZ bit.
        unsafe { std::arch::asm!("ldmxcsr [{}]",in(reg)&value,options(nostack,preserves_flags)) };
        Self(old)
    }
}
impl Drop for Ftz {
    fn drop(&mut self) {
        // SAFETY: restore exactly the saved MXCSR value.
        unsafe { std::arch::asm!("ldmxcsr [{}]",in(reg)&self.0,options(nostack,preserves_flags)) };
    }
}
fn compare_prefix(block: &Block, interleaved: &[u32], label: &str) -> usize {
    match (
        block.output_prefix_frames,
        &block.output_prefix_interleaved_bits,
    ) {
        (None, None) => 0,
        (Some(frames), Some(bits)) => {
            assert!(frames <= block.output_frames, "{label} prefix bounds");
            assert_eq!(bits.len(), frames * 2, "{label} complete prefix");
            assert_eq!(&interleaved[..frames * 2], bits, "{label} selected PCM");
            1
        }
        _ => panic!("{label} incomplete selected prefix receipt"),
    }
}
fn compare(
    compressed: &[u8],
    expected_cases: usize,
    expected_blocks: usize,
    expected_prefixes: usize,
) {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(compressed)
        .take(16 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .unwrap();
    let corpus: Corpus = serde_json::from_slice(&bytes).unwrap();
    assert!(corpus.schema.starts_with("cumulus-full-local-stereo-"));
    assert!(corpus.full_sdk);
    assert!(!corpus.real_windows);
    assert!(!corpus.original_fifo);
    assert!(!corpus.dsp_replaced);
    assert_eq!(corpus.cases.len(), expected_cases);
    let mut calls = 0;
    let mut prefixes = 0;
    for case in corpus.cases {
        let _ftz = Ftz::off();
        let label = format!("{} rate{} cap{}", case.name, case.rate, case.capacity);
        let format = Format::new(case.rate as f32, 2, case.capacity).unwrap();
        let mut graph =
            LocalFixedStereoMaterial::new(format, plan(case.name != "default_all_off")).unwrap();
        let mut pcm = PlanarBlock::new(format);
        let mut total = 0;
        for (index, block) in case.blocks.iter().enumerate() {
            assert_eq!(block.input_interleaved_bits.len(), block.input_frames * 2);
            pcm.samples_mut().fill(0.0);
            for (frame, pair) in block.input_interleaved_bits.chunks_exact(2).enumerate() {
                pcm.samples_mut()[frame] = f32::from_bits(pair[0]);
                pcm.samples_mut()[format.capacity() + frame] = f32::from_bits(pair[1]);
            }
            match block.action_before {
                Some(Action::Disable) => graph.configure(plan(false)).unwrap(),
                Some(Action::Reset) => graph.reset().unwrap(),
                None => (),
            }
            let flow = graph
                .process(&mut pcm, block.input_frames, block.input_end)
                .unwrap();
            assert_eq!(
                (flow.valid_frames, flow.end),
                (block.output_frames, block.output_end),
                "{label} block{index} n/end"
            );
            let interleaved = (0..flow.valid_frames)
                .flat_map(|i| {
                    [
                        pcm.samples()[i].to_bits(),
                        pcm.samples()[format.capacity() + i].to_bits(),
                    ]
                })
                .collect::<Vec<_>>();
            let raw = interleaved
                .iter()
                .flat_map(|b| b.to_le_bytes())
                .collect::<Vec<_>>();
            assert_eq!(
                hex::encode(Sha256::digest(raw)),
                block.output_sha256,
                "{label} block{index} all valid PCM"
            );
            // Every block's complete valid-PCM SHA above is mandatory. The
            // compact original corpus additionally stores raw prefix bits on
            // selected blocks only; missing paired fields are intentional.
            prefixes += compare_prefix(block, &interleaved, &format!("{label} block{index}"));
            let expected_order = [
                "0x8", "0x18", "0x58", "0x48", "0x50", "0x70", "0x68", "0x60", "0xa0", "0x90",
                "0x98", "0x28", "0x30", "0x40", "0x78", "0x20", "0xa8", "0x38", "0xb0", "0x10",
            ]
            .into_iter()
            .filter(|slot| {
                (*slot != "0x8" || block.input_frames > 0)
                    && (case.rate <= 48000 || !matches!(*slot, "0xa0" | "0xa8"))
            })
            .collect::<Vec<_>>();
            assert_eq!(
                block.child_order, expected_order,
                "{label} block{index} observed original order"
            );
            assert_eq!(mxcsr() & 0x8000, 0);
            total += flow.valid_frames as u64;
            calls += 1;
        }
        assert_eq!(total, case.output_frames, "{label} total output frames");
    }
    assert_eq!(calls, expected_blocks);
    assert_eq!(prefixes, expected_prefixes);
}
#[test]
fn full_original_fixed_scope_pcm_off_edges_seek_and_eof() {
    compare(
        include_bytes!("fixtures/effects-local-stereo-full-official.json.gz"),
        24,
        1002,
        78,
    );
}
#[test]
fn first_short_or_empty_eof_keeps_real_environment_tail_and_limiter_protocol() {
    compare(
        include_bytes!("fixtures/effects-local-stereo-first-eof-official.json.gz"),
        4,
        92,
        12,
    );
}
#[test]
fn unsupported_formats_and_invalid_pcm_do_not_change_the_fixed_scope() {
    for (rate, channels, capacity) in [(32000.0, 2, 2048), (44100.0, 1, 2048), (48000.0, 2, 1024)] {
        assert!(
            LocalFixedStereoMaterial::new(
                Format::new(rate, channels, capacity).unwrap(),
                plan(true)
            )
            .is_err()
        );
    }
    let format = Format::new(44100.0, 2, 2048).unwrap();
    let mut graph = LocalFixedStereoMaterial::new(format, plan(true)).unwrap();
    let mut reference = LocalFixedStereoMaterial::new(format, plan(true)).unwrap();
    let mut pcm = PlanarBlock::new(format);
    pcm.samples_mut()[0] = f32::NAN;
    assert!(graph.process(&mut pcm, 1, false).is_err());
    pcm.samples_mut().fill(0.0);
    pcm.samples_mut()[0] = 0.25;
    let mut expected = PlanarBlock::new(format);
    expected.samples_mut().copy_from_slice(pcm.samples());
    let a = graph.process(&mut pcm, 7, true).unwrap();
    let b = reference.process(&mut expected, 7, true).unwrap();
    assert_eq!((a.valid_frames, a.end), (b.valid_frames, b.end));
    assert_eq!(pcm.samples(), expected.samples());
}

#[derive(Deserialize)]
struct SliderCorpus {
    cases: Vec<SliderCase>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SliderCase {
    rate: u32,
    capacity: u32,
    output_frames: u64,
    blocks: Vec<SliderBlock>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SliderBlock {
    input_frames: usize,
    input_end: bool,
    input_interleaved_bits: Vec<u32>,
    action_before: Option<Vec<SliderAction>>,
    output_frames: usize,
    output_end: bool,
    output_sha256: String,
}
#[derive(Deserialize)]
#[serde(tag = "kind")]
enum SliderAction {
    #[serde(rename = "original_se_controls")]
    Controls { presence: f32, stereoizer: f32 },
    #[serde(rename = "original_se_enable")]
    Enable { enabled: bool },
    #[serde(rename = "original_graph_seek_reset")]
    Reset,
}
fn selected_plan(enabled: bool, presence: i32, stereoizer: i32) -> LocalStereoMaterialPlan {
    if !enabled {
        return plan(false);
    }
    LocalStereoMaterialPlan::from_equalizer_json(&json!({"eq":{"on":false,"eqs":vec![0;10]},"bt":{"on":false,"bass":0,"treble":0},"rvb":{"on":false},"se":{"on":true,"presence":presence,"stereoizer":stereoizer,"sshaper":true,"ambience":1}})).unwrap()
}
#[test]
fn full_original_cold_bank_switch_hot_controls_and_both_pending_reset_edges() {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("fixtures/effects-local-stereo-slider-official.json.gz")[..],
    )
    .take(8 * 1024 * 1024)
    .read_to_end(&mut bytes)
    .unwrap();
    let corpus: SliderCorpus = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(corpus.cases.len(), 2);
    let mut calls = 0;
    for case in corpus.cases {
        let _ftz = Ftz::off();
        let format = Format::new(case.rate as f32, 2, case.capacity).unwrap();
        let mut graph = LocalFixedStereoMaterial::new(format, plan(false)).unwrap();
        let mut pcm = PlanarBlock::new(format);
        let (mut enabled, mut presence, mut stereoizer) = (false, 0, 0);
        let mut total = 0;
        for (index, block) in case.blocks.iter().enumerate() {
            for action in block.action_before.as_deref().unwrap_or_default() {
                match action {
                    SliderAction::Controls {
                        presence: p,
                        stereoizer: s,
                    } => {
                        assert_eq!(p.fract(), 0.0);
                        assert_eq!(s.fract(), 0.0);
                        presence = *p as i32;
                        stereoizer = *s as i32;
                        if enabled {
                            graph
                                .configure(selected_plan(true, presence, stereoizer))
                                .unwrap();
                        }
                    }
                    SliderAction::Enable { enabled: next } => {
                        enabled = *next;
                        graph
                            .configure(selected_plan(enabled, presence, stereoizer))
                            .unwrap();
                    }
                    SliderAction::Reset => graph.reset().unwrap(),
                }
            }
            let input = block
                .input_interleaved_bits
                .iter()
                .map(|b| f32::from_bits(*b))
                .collect::<Vec<_>>();
            assert_eq!(pcm.load_interleaved(&input).unwrap(), block.input_frames);
            let flow = graph
                .process(&mut pcm, block.input_frames, block.input_end)
                .unwrap();
            assert_eq!(
                (flow.valid_frames, flow.end),
                (block.output_frames, block.output_end),
                "rate{} block{index}",
                case.rate
            );
            let bytes = (0..flow.valid_frames)
                .flat_map(|i| [pcm.samples()[i], pcm.samples()[format.capacity() + i]])
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>();
            assert_eq!(
                hex::encode(Sha256::digest(bytes)),
                block.output_sha256,
                "rate{} block{index} every PCM",
                case.rate
            );
            total += flow.valid_frames as u64;
            calls += 1;
        }
        assert_eq!(total, case.output_frames);
    }
    assert_eq!(calls, 60);
}

#[derive(Deserialize)]
struct AfterEofCorpus {
    #[serde(rename = "sameOriginalInstance")]
    same_instance: bool,
    phases: Vec<Case>,
}
#[test]
fn full_original_after_eof_reset_and_new_non_eof_audio_rearm_tail_on_same_instance() {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("fixtures/effects-local-stereo-after-eof-reset-official.json.gz")[..],
    )
    .take(8 * 1024 * 1024)
    .read_to_end(&mut bytes)
    .unwrap();
    let corpus: AfterEofCorpus = serde_json::from_slice(&bytes).unwrap();
    assert!(corpus.same_instance);
    assert_eq!(corpus.phases.len(), 2);
    let _ftz = Ftz::off();
    let format = Format::new(48000.0, 2, 2048).unwrap();
    let mut graph = LocalFixedStereoMaterial::new(format, plan(true)).unwrap();
    let mut pcm = PlanarBlock::new(format);
    let mut calls = 0;
    for (phase, case) in corpus.phases.iter().enumerate() {
        let mut total = 0;
        assert_eq!((case.rate, case.capacity), (48000, 2048));
        for (index, block) in case.blocks.iter().enumerate() {
            if let Some(Action::Reset) = block.action_before {
                graph.reset().unwrap();
            }
            if phase == 1 && index == 0 {
                assert!(matches!(block.action_before, Some(Action::Reset)));
                assert_eq!(block.input_frames, 512);
                assert!(!block.input_end);
            }
            let input = block
                .input_interleaved_bits
                .iter()
                .map(|b| f32::from_bits(*b))
                .collect::<Vec<_>>();
            assert_eq!(pcm.load_interleaved(&input).unwrap(), block.input_frames);
            let flow = graph
                .process(&mut pcm, block.input_frames, block.input_end)
                .unwrap();
            assert_eq!(
                (flow.valid_frames, flow.end),
                (block.output_frames, block.output_end),
                "phase{phase} block{index}"
            );
            let bytes = (0..flow.valid_frames)
                .flat_map(|i| [pcm.samples()[i], pcm.samples()[format.capacity() + i]])
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>();
            assert_eq!(
                hex::encode(Sha256::digest(bytes)),
                block.output_sha256,
                "phase{phase} block{index} every PCM"
            );
            total += flow.valid_frames as u64;
            calls += 1;
        }
        assert_eq!(total, case.output_frames);
        assert_eq!(total, if phase == 0 { 50055 } else { 48519 });
    }
    assert_eq!(calls, 50);
}
