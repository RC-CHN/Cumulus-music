//! Real original SE1 -> B0 serial subgraph, explicitly not the full SDK.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
use cumulus_core::effects::{
    dsp::{Format, PlanarBlock},
    stereo::{LocalStereoMaterialPlan, LocalStereoSubgraph},
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::Read;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Block {
    frames: usize,
    end: bool,
    reset: bool,
    input_bits: Vec<u32>,
    output_frames: usize,
    output_end: bool,
    output_sha256: String,
    output_bits: Option<Vec<u32>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    rate: u32,
    capacity: u32,
    caller_ftz: u32,
    blocks: Vec<Block>,
    se_first_end_block: usize,
    limiter_first_end_block: usize,
    output_frames: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Corpus {
    schema: String,
    full_sdk_graph: bool,
    cases: Vec<Case>,
}
fn mxcsr() -> u32 {
    let mut value = 0u32;
    // SAFETY: valid x86_64 four-byte MXCSR destination.
    unsafe { std::arch::asm!("stmxcsr [{}]",in(reg)&mut value,options(nostack,preserves_flags)) };
    value
}
fn set_mxcsr(value: u32) {
    // SAFETY: preserve every caller bit and only set/restore the valid FTZ bit.
    unsafe { std::arch::asm!("ldmxcsr [{}]",in(reg)&value,options(nostack,preserves_flags)) };
}
struct Ftz(u32);
impl Ftz {
    fn off() -> Self {
        let old = mxcsr();
        set_mxcsr(old & !0x8000);
        Self(old)
    }
}
impl Drop for Ftz {
    fn drop(&mut self) {
        set_mxcsr(self.0);
    }
}
#[test]
fn composed_pcm_reset_and_limiter_eof_after_environment_match_original_functions() {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("fixtures/effects-stereo-material-official.json.gz")[..],
    )
    .take(32 * 1024 * 1024)
    .read_to_end(&mut bytes)
    .unwrap();
    let corpus: Corpus = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(corpus.schema, "cumulus-local-stereo-subgraph-v1");
    assert!(!corpus.full_sdk_graph);
    assert_eq!(corpus.cases.len(), 12);
    let plan=LocalStereoMaterialPlan::from_equalizer_json(&json!({"eq":{"on":false,"eqs":vec![0;10]},"bt":{"on":false,"bass":0,"treble":0},"rvb":{"on":false},"se":{"on":true,"presence":3,"stereoizer":5,"sshaper":true,"ambience":1}})).unwrap();
    let mut calls = 0;
    let mut total = 0u64;
    let mut delayed_eof = 0;
    for case in corpus.cases {
        assert_eq!(case.caller_ftz, 0);
        let _ftz = Ftz::off();
        let format = Format::new(case.rate as f32, 2, case.capacity).unwrap();
        let mut graph = LocalStereoSubgraph::new(format, plan).unwrap();
        let mut pcm = PlanarBlock::new(format);
        let mut output = 0u64;
        delayed_eof += usize::from(case.limiter_first_end_block > case.se_first_end_block);
        for (index, block) in case.blocks.iter().enumerate() {
            assert_eq!(block.input_bits.len(), format.sample_count());
            for (to, bits) in pcm.samples_mut().iter_mut().zip(&block.input_bits) {
                *to = f32::from_bits(*bits);
            }
            if block.reset {
                graph.reset().unwrap();
            }
            let flow = graph.process(&mut pcm, block.frames, block.end).unwrap();
            assert_eq!(
                (flow.valid_frames, flow.end),
                (block.output_frames, block.output_end),
                "{} block{index} frame/EOF",
                case.name
            );
            let bits = pcm.samples()[..flow.valid_frames]
                .iter()
                .chain(&pcm.samples()[format.capacity()..format.capacity() + flow.valid_frames])
                .map(|x| x.to_bits())
                .collect::<Vec<_>>();
            if let Some(expected) = &block.output_bits {
                assert_eq!(&bits, expected, "{} block{index} PCM", case.name);
            }
            let raw = bits
                .iter()
                .flat_map(|x| x.to_le_bytes())
                .collect::<Vec<_>>();
            assert_eq!(
                hex::encode(Sha256::digest(raw)),
                block.output_sha256,
                "{} block{index} every PCM receipt",
                case.name
            );
            assert_eq!(mxcsr() & 0x8000, 0);
            output += flow.valid_frames as u64;
            calls += 1;
        }
        assert_eq!(output, case.output_frames);
        total += output;
    }
    assert_eq!(calls, 1032);
    assert_eq!(total, 796718);
    assert_eq!(delayed_eof, 6);
}
