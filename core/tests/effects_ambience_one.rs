//! Original fixed-bank1 child vectors and the owned-arena boundary.
//! No binary, material blob, account, decoder or sound device at test runtime.
use cumulus_core::effects::dsp::*;
#[path = "../examples/support/ambience_one_wire.rs"]
mod wire;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod original {
    use super::*;
    use serde::Deserialize;
    use sha2::{Digest, Sha256};
    use std::{io::Read, sync::OnceLock};

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Block {
        frames: usize,
        end: bool,
        reset: bool,
        input_bits: Vec<u32>,
        output_frames: usize,
        output_end: bool,
        output_bits: Option<Vec<u32>>,
        output_sha256: String,
        indices: Vec<u32>,
        history_bits: Vec<u32>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Case {
        name: String,
        rate: u32,
        capacity: u32,
        caller_ftz: u32,
        blocks: Vec<Block>,
    }
    #[derive(Deserialize)]
    struct Corpus {
        schema: String,
        cases: Vec<Case>,
    }
    fn corpus() -> &'static Corpus {
        static CORPUS: OnceLock<Corpus> = OnceLock::new();
        CORPUS.get_or_init(|| {
            let mut bytes = Vec::new();
            flate2::read::GzDecoder::new(
                &include_bytes!("fixtures/effects-ambience-one-official.json.gz")[..],
            )
            .take(32 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .unwrap();
            serde_json::from_slice(&bytes).unwrap()
        })
    }
    fn mxcsr() -> u32 {
        let mut value = 0u32;
        // SAFETY: x86_64 MXCSR read into the four-byte local value.
        unsafe {
            std::arch::asm!("stmxcsr [{}]",in(reg)&mut value,options(nostack,preserves_flags));
        }
        value
    }
    fn set_mxcsr(value: u32) {
        // SAFETY: only the valid caller MXCSR FTZ bit is changed/restored.
        unsafe {
            std::arch::asm!("ldmxcsr [{}]",in(reg)&value,options(nostack,preserves_flags));
        }
    }
    struct Ftz(u32);
    impl Ftz {
        fn new(enabled: u32) -> Self {
            let value = mxcsr();
            set_mxcsr((value & !0x8000) | if enabled == 1 { 0x8000 } else { 0 });
            Self(value)
        }
    }
    impl Drop for Ftz {
        fn drop(&mut self) {
            set_mxcsr(self.0);
        }
    }
    fn state(stage: &AmbienceOneEnvironment) -> (Vec<u32>, Vec<u32>) {
        let value = stage.snapshot().unwrap();
        let indices = [
            value.remaining_tail.unwrap_or(u32::MAX),
            value.last_tail,
            value.total_tail,
        ]
        .into_iter()
        .chain(value.diffusion_positions)
        .chain(value.feedback_positions.into_iter().flatten())
        .collect();
        let history = value
            .damping
            .into_iter()
            .flatten()
            .chain(value.feedback_history.into_iter().flatten())
            .map(f32::to_bits)
            .collect();
        (indices, history)
    }
    #[test]
    fn independent_init_pcm_state_reset_short_blocks_and_full_tail_match_original() {
        assert_eq!(corpus().schema, "cumulus-se-bank1-environment-v1");
        assert_eq!(corpus().cases.len(), 14);
        let mut count = 0;
        let mut audible_tail = false;
        for case in &corpus().cases {
            let _ftz = Ftz::new(case.caller_ftz);
            let format = Format::new(case.rate as f32, 2, case.capacity).unwrap();
            let mut stage = AmbienceOneEnvironment::new(format).unwrap();
            let mut block = PlanarBlock::new(format);
            for (index, input) in case.blocks.iter().enumerate() {
                assert_eq!(input.input_bits.len(), format.sample_count());
                for (sample, bits) in block.samples_mut().iter_mut().zip(&input.input_bits) {
                    *sample = f32::from_bits(*bits);
                }
                if input.reset {
                    stage.reset().unwrap();
                }
                let result = stage.process(&mut block, input.frames, input.end).unwrap();
                assert_eq!(
                    result.valid_frames, input.output_frames,
                    "{} block{index}",
                    case.name
                );
                assert_eq!(result.end, input.output_end, "{} block{index}", case.name);
                let actual = block.samples()[..result.valid_frames]
                    .iter()
                    .chain(
                        &block.samples()
                            [format.capacity()..format.capacity() + result.valid_frames],
                    )
                    .map(|f| f.to_bits())
                    .collect::<Vec<_>>();
                if let Some(expected) = &input.output_bits {
                    assert_eq!(&actual, expected, "{} block{index} PCM", case.name);
                }
                let raw = actual
                    .iter()
                    .flat_map(|x| x.to_le_bytes())
                    .collect::<Vec<_>>();
                assert_eq!(
                    hex::encode(Sha256::digest(raw)),
                    input.output_sha256,
                    "{} block{index} all PCM",
                    case.name
                );
                let (indices, history) = state(&stage);
                assert_eq!(
                    indices, input.indices,
                    "{} block{index} cursors/tail",
                    case.name
                );
                assert_eq!(
                    history, input.history_bits,
                    "{} block{index} histories",
                    case.name
                );
                assert_eq!(u32::from(mxcsr() & 0x8000 != 0), case.caller_ftz);
                audible_tail |= input.frames == 0
                    && input.end
                    && actual.iter().any(|x| f32::from_bits(*x).abs() > 0.000001);
                count += 1;
            }
        }
        assert_eq!(count, 1764);
        assert!(
            audible_tail,
            "the frozen path must exercise actual reverberation"
        );
    }
}

#[test]
fn unsupported_shapes_nonfinite_input_and_capacity_errors_leave_state_unchanged() {
    for (rate, channels, capacity) in [
        (48000.0, 1, 64),
        (48000.0, 2, 1),
        (48000.0, 2, 7),
        (44100.5, 2, 64),
    ] {
        assert!(
            AmbienceOneEnvironment::new(Format::new(rate, channels, capacity).unwrap()).is_err()
        );
    }
    let format = Format::new(48000.0, 2, 64).unwrap();
    let mut stage = AmbienceOneEnvironment::new(format).unwrap();
    let mut block = PlanarBlock::new(format);
    let initial = stage.snapshot().unwrap();
    assert!(stage.process(&mut block, 65, false).is_err());
    block.samples_mut()[0] = f32::NAN;
    assert!(stage.process(&mut block, 64, false).is_err());
    assert_eq!(stage.snapshot().unwrap(), initial);
    assert!(
        stage
            .process(
                &mut PlanarBlock::new(Format::new(48000.0, 2, 128).unwrap()),
                64,
                false
            )
            .is_err()
    );
    assert_eq!(stage.snapshot().unwrap(), initial);
    block.samples_mut().fill(0.0);
    assert_eq!(
        stage.process(&mut block, 0, false).unwrap(),
        AmbienceOneResult {
            valid_frames: 0,
            end: false
        }
    );
    assert_eq!(stage.snapshot().unwrap(), initial);
    stage.process(&mut block, 64, false).unwrap();
}

#[test]
fn reset_preserves_positions_and_one_second_tail_budget_and_eof_eventually_stops() {
    let format = Format::new(8000.0, 2, 64).unwrap();
    let mut stage = AmbienceOneEnvironment::new(format).unwrap();
    let mut block = PlanarBlock::new(format);
    block.samples_mut()[0] = 0.5;
    stage.process(&mut block, 63, false).unwrap();
    block.samples_mut().fill(0.0);
    let first = stage.process(&mut block, 0, true).unwrap();
    assert_eq!(
        first,
        AmbienceOneResult {
            valid_frames: 64,
            end: false
        }
    );
    let prior = stage.snapshot().unwrap();
    stage.reset().unwrap();
    let reset = stage.snapshot().unwrap();
    assert_eq!(reset.diffusion_positions, prior.diffusion_positions);
    assert_eq!(reset.feedback_positions, prior.feedback_positions);
    assert_eq!(reset.remaining_tail, prior.remaining_tail);
    assert_eq!(reset.last_tail, prior.last_tail);
    assert!(reset.feedback_history.iter().flatten().all(|x| *x == 0.0));
    assert!(reset.damping.iter().all(|x| x[0] == 0.0 && x[1] == 0.0));
    let mut tail = first.valid_frames;
    for _ in 0..126 {
        block.samples_mut().fill(0.0);
        let result = stage.process(&mut block, 0, true).unwrap();
        tail += result.valid_frames;
        if result.end {
            break;
        }
    }
    assert_eq!(tail, 8000);
    assert_eq!(stage.snapshot().unwrap().remaining_tail, Some(0));
    assert_eq!(
        stage.process(&mut block, 0, true).unwrap(),
        AmbienceOneResult {
            valid_frames: 0,
            end: true
        }
    );
}

#[test]
fn numerical_failure_silences_owned_block_and_quarantines_environment() {
    let format = Format::new(48000.0, 2, 64).unwrap();
    let mut stage = AmbienceOneEnvironment::new(format).unwrap();
    let mut block = PlanarBlock::new(format);
    block.samples_mut().fill(f32::MAX);
    assert!(stage.process(&mut block, 64, false).is_err());
    assert!(block.samples().iter().all(|x| *x == 0.0));
    assert!(stage.reset().is_err());
    assert!(stage.process(&mut block, 0, true).is_err());
}

#[test]
fn explicit_component_cli_handles_interleaved_input_and_zero_frame_tail_drain() {
    let request = br#"{"sampleRate":8000,"maxFrames":64,"blocks":[{"samples":[0.5,-0.25]},{"samples":[],"end":true}]}"#;
    let value = wire::render(request).unwrap();
    assert_eq!(value["ambience"], 1);
    assert_eq!(value["bank"], 1);
    assert_eq!(value["completeGraph"], false);
    let format = Format::new(8000.0, 2, 64).unwrap();
    let mut stage = AmbienceOneEnvironment::new(format).unwrap();
    let mut block = PlanarBlock::new(format);
    for (index, (input, end)) in [(&[0.5, -0.25][..], false), (&[][..], true)]
        .into_iter()
        .enumerate()
    {
        let frames = block.load_interleaved(input).unwrap();
        let result = stage.process(&mut block, frames, end).unwrap();
        let mut samples = vec![0.0; 2 * result.valid_frames];
        block
            .copy_interleaved(result.valid_frames, &mut samples)
            .unwrap();
        let actual = value["blocks"][index]["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| (x.as_f64().unwrap() as f32).to_bits())
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            samples.into_iter().map(f32::to_bits).collect::<Vec<_>>()
        );
        assert_eq!(
            value["blocks"][index]["timing"]["validFrames"],
            result.valid_frames
        );
        assert_eq!(value["blocks"][index]["timing"]["end"], result.end);
    }
    assert!(
        wire::render(
            br#"{"sampleRate":8000,"maxFrames":64,"blocks":[{"samples":[],"ambience":2}]}"#
        )
        .is_err()
    );
}
