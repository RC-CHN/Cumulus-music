//! Fixed-bank1 full SE tests. No service, binary or sound device at runtime.
use cumulus_core::effects::dsp::*;
#[path = "../examples/support/stereo_one_wire.rs"]
mod wire;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod original {
    use super::*;
    use serde::Deserialize;
    use sha2::{Digest, Sha256};
    use std::io::Read;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Input {
        frames: usize,
        end: bool,
        reset: bool,
        input_bits: Vec<u32>,
    }
    #[derive(Deserialize)]
    struct Inputs {
        name: String,
        blocks: Vec<Input>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Block {
        presence: f32,
        stereoizer: f32,
        shaper: bool,
        bypass: bool,
        output_frames: usize,
        output_end: bool,
        output_bits: Option<Vec<u32>>,
        output_sha256: String,
        parent_bits: Vec<u32>,
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
    #[serde(rename_all = "camelCase")]
    struct Corpus {
        schema: String,
        input_fixture_sha256: String,
        cases: Vec<Case>,
    }
    #[derive(Deserialize)]
    struct InputCorpus {
        cases: Vec<Inputs>,
    }
    fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> T {
        let mut plain = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .take(32 * 1024 * 1024)
            .read_to_end(&mut plain)
            .unwrap();
        serde_json::from_slice(&plain).unwrap()
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
        // SAFETY: only caller MXCSR's valid FTZ bit is changed/restored.
        unsafe {
            std::arch::asm!("ldmxcsr [{}]",in(reg)&value,options(nostack,preserves_flags));
        }
    }
    struct Ftz(u32);
    impl Ftz {
        fn new(enabled: u32) -> Self {
            let old = mxcsr();
            set_mxcsr((old & !0x8000) | if enabled == 1 { 0x8000 } else { 0 });
            Self(old)
        }
    }
    impl Drop for Ftz {
        fn drop(&mut self) {
            set_mxcsr(self.0);
        }
    }
    #[test]
    fn fixed_bank1_full_parent_matches_original_pcm_gain_state_and_tail() {
        let raw_inputs = include_bytes!("fixtures/effects-ambience-one-official.json.gz");
        let inputs: InputCorpus = decode(raw_inputs);
        let corpus: Corpus = decode(include_bytes!(
            "fixtures/effects-stereo-one-official.json.gz"
        ));
        assert_eq!(corpus.schema, "cumulus-se-bank1-parent-v1");
        assert_eq!(
            hex::encode(Sha256::digest(raw_inputs)),
            corpus.input_fixture_sha256
        );
        assert_eq!(corpus.cases.len(), 14);
        let mut count = 0;
        let mut audible_tail = false;
        for case in corpus.cases {
            let source = inputs.cases.iter().find(|c| c.name == case.name).unwrap();
            assert_eq!(case.blocks.len(), source.blocks.len());
            let _ftz = Ftz::new(case.caller_ftz);
            let format = Format::new(case.rate as f32, 2, case.capacity).unwrap();
            let mut stage = StereoAmbienceOneStage::new(format).unwrap();
            let mut pcm = PlanarBlock::new(format);
            for (index, (action, input)) in case.blocks.iter().zip(&source.blocks).enumerate() {
                for (sample, bits) in pcm.samples_mut().iter_mut().zip(&input.input_bits) {
                    *sample = f32::from_bits(*bits);
                }
                stage
                    .set_controls(StereoAmbienceOneControls {
                        presence: action.presence,
                        stereoizer: action.stereoizer,
                        spectrum_shaper: action.shaper,
                    })
                    .unwrap();
                stage.set_bypass(action.bypass).unwrap();
                if input.reset {
                    stage.reset().unwrap();
                }
                let timing = stage.process(&mut pcm, input.frames, input.end).unwrap();
                assert_eq!(
                    (timing.valid_frames, timing.end),
                    (action.output_frames, action.output_end),
                    "{} block{index} flow",
                    case.name
                );
                let bits = pcm.samples()[..timing.valid_frames]
                    .iter()
                    .chain(
                        &pcm.samples()[format.capacity()..format.capacity() + timing.valid_frames],
                    )
                    .map(|x| x.to_bits())
                    .collect::<Vec<_>>();
                if let Some(expected) = &action.output_bits {
                    assert_eq!(&bits, expected, "{} block{index} PCM", case.name);
                }
                let raw = bits
                    .iter()
                    .flat_map(|x| x.to_le_bytes())
                    .collect::<Vec<_>>();
                assert_eq!(
                    hex::encode(Sha256::digest(raw)),
                    action.output_sha256,
                    "{} block{index} PCM receipt",
                    case.name
                );
                let state = stage.snapshot().unwrap();
                let parent = [state.side_gain, state.applied_presence]
                    .into_iter()
                    .chain(state.detector_history.into_iter().flatten())
                    .map(f32::to_bits)
                    .collect::<Vec<_>>();
                assert_eq!(
                    parent, action.parent_bits,
                    "{} block{index} parent",
                    case.name
                );
                let env = state.environment;
                let indices = [
                    env.remaining_tail.unwrap_or(u32::MAX),
                    env.last_tail,
                    env.total_tail,
                ]
                .into_iter()
                .chain(env.diffusion_positions)
                .chain(env.feedback_positions.into_iter().flatten())
                .collect::<Vec<_>>();
                let history = env
                    .damping
                    .into_iter()
                    .flatten()
                    .chain(env.feedback_history.into_iter().flatten())
                    .map(f32::to_bits)
                    .collect::<Vec<_>>();
                assert_eq!(
                    indices, action.indices,
                    "{} block{index} cursors",
                    case.name
                );
                assert_eq!(
                    history, action.history_bits,
                    "{} block{index} env",
                    case.name
                );
                assert_eq!(u32::from(mxcsr() & 0x8000 != 0), case.caller_ftz);
                audible_tail |= input.frames == 0
                    && input.end
                    && bits.iter().any(|x| f32::from_bits(*x).abs() > 0.000001);
                count += 1;
            }
        }
        assert_eq!(count, 1764);
        assert!(audible_tail);
    }

    #[test]
    fn actual_cli_valid_pcm_matches_the_original_first_five_gestures() {
        let inputs: InputCorpus = decode(include_bytes!(
            "fixtures/effects-ambience-one-official.json.gz"
        ));
        let corpus: Corpus = decode(include_bytes!(
            "fixtures/effects-stereo-one-official.json.gz"
        ));
        let case = &corpus.cases[0];
        let source = inputs.cases.iter().find(|x| x.name == case.name).unwrap();
        let _ftz = Ftz::new(case.caller_ftz);
        let requests = source.blocks.iter().zip(&case.blocks).take(5).map(|(input,action)| {
            let samples=(0..input.frames).flat_map(|i|[f32::from_bits(input.input_bits[i]),f32::from_bits(input.input_bits[case.capacity as usize+i])]).collect::<Vec<_>>();
            serde_json::json!({"samples":samples,"controls":{"presence":action.presence,"stereoizer":action.stereoizer,"sshaper":action.shaper},"bypass":action.bypass,"reset":input.reset,"end":input.end})
        }).collect::<Vec<_>>();
        let value = wire::render(&serde_json::to_vec(&serde_json::json!({"sampleRate":case.rate,"maxFrames":case.capacity,"blocks":requests})).unwrap()).unwrap();
        for (index, action) in case.blocks.iter().take(5).enumerate() {
            let actual = value["blocks"][index]["samples"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| (v.as_f64().unwrap() as f32).to_bits())
                .collect::<Vec<_>>();
            let expected = action.output_bits.as_ref().unwrap();
            let expected = (0..action.output_frames)
                .flat_map(|i| [expected[i], expected[action.output_frames + i]])
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "original CLI block{index}");
        }
    }
}

#[test]
fn zero_audio_advances_only_internal_se_detector_and_does_not_change_public_zero_contracts() {
    let format = Format::new(48000.0, 2, 64).unwrap();
    let mut stage = StereoAmbienceOneStage::new(format).unwrap();
    let mut pcm = PlanarBlock::new(format);
    let initial = stage.snapshot().unwrap();
    let timing = stage.process(&mut pcm, 0, false).unwrap();
    assert_eq!(
        timing,
        AmbienceOneResult {
            valid_frames: 0,
            end: false
        }
    );
    let after = stage.snapshot().unwrap();
    assert_eq!(after.environment, initial.environment);
    assert_eq!(after.side_gain, 1.0);
    for channel in after.detector_history {
        for pair in channel.chunks_exact(2) {
            assert_eq!(pair[0], 0.0);
            assert!(pair[1] > 0.0);
        }
    }
    let mut zero = StereoNoAmbienceStage::new(format).unwrap();
    let before = zero.snapshot().unwrap();
    assert!(zero.process(&mut pcm, 0, false).is_err());
    assert_eq!(zero.snapshot().unwrap(), before);
    let mono = Format::new(48000.0, 1, 64).unwrap();
    let parameters = DynamicsParameters::new(DynamicsSettings {
        bands: [DynamicsBand {
            input_gain_db: 0.0,
            threshold_db: -20.0,
            ratio: 1.2,
            attack_seconds: 0.1,
            release_seconds: 0.2,
            minimum_gain_db: -6.0,
            output_gain_db: 0.0,
        }; 5],
        crossover_hz: [100.0, 350.0, 1500.0, 7500.0],
        output_gain_db: 0.0,
        process_lfe: true,
        link_channels: true,
        fourth_order: false,
        band_count: 5,
        input_gain_db: 0.0,
    })
    .unwrap();
    let mut dynamics = MultibandDynamics::new(mono, &parameters).unwrap();
    let mut main = PlanarBlock::new(mono);
    main.samples_mut().fill(0.25);
    dynamics.process(&mut main, None, 0).unwrap();
    assert_eq!(dynamics.detector_history().unwrap(), [0.0; 10]);
    assert!(main.samples().iter().all(|x| *x == 0.25));
}

#[test]
fn bypass_is_explicit_and_stable_bypass_does_not_advance_environment() {
    let format = Format::new(48000.0, 2, 64).unwrap();
    let mut stage = StereoAmbienceOneStage::new(format).unwrap();
    let mut pcm = PlanarBlock::new(format);
    stage.set_bypass(true).unwrap();
    stage.process(&mut pcm, 64, false).unwrap();
    let before = stage.snapshot().unwrap();
    pcm.load_interleaved(&[0.125, -0.25]).unwrap();
    let original = pcm.samples().to_vec();
    assert_eq!(stage.process(&mut pcm, 1, false).unwrap().valid_frames, 1);
    assert_eq!(pcm.samples(), original);
    assert_eq!(stage.snapshot().unwrap(), before);
    stage.set_bypass(false).unwrap();
    assert!(
        stage
            .snapshot()
            .unwrap()
            .detector_history
            .iter()
            .flatten()
            .all(|x| *x == 0.0)
    );
}

#[test]
fn invalid_input_is_atomic_and_numeric_failure_requires_recreation() {
    for (rate, channels, capacity) in [(44100.5, 2, 64), (48000.0, 1, 64), (48000.0, 2, 7)] {
        assert!(
            StereoAmbienceOneStage::new(Format::new(rate, channels, capacity).unwrap()).is_err()
        );
    }
    let format = Format::new(48000.0, 2, 64).unwrap();
    let mut stage = StereoAmbienceOneStage::new(format).unwrap();
    let initial = stage.snapshot().unwrap();
    for (presence, stereoizer) in [
        (f32::NAN, 0.0),
        (0.0, f32::INFINITY),
        (-10.1, 0.0),
        (0.0, 10.1),
    ] {
        assert!(
            stage
                .set_controls(StereoAmbienceOneControls {
                    presence,
                    stereoizer,
                    spectrum_shaper: true
                })
                .is_err()
        );
        assert_eq!(stage.snapshot().unwrap(), initial);
    }
    let mut pcm = PlanarBlock::new(format);
    pcm.samples_mut()[0] = f32::NAN;
    assert!(stage.process(&mut pcm, 64, false).is_err());
    assert_eq!(stage.snapshot().unwrap(), initial);
    pcm.samples_mut().fill(f32::MAX);
    assert!(stage.process(&mut pcm, 64, false).is_err());
    assert!(pcm.samples().iter().all(|x| *x == 0.0));
    assert!(stage.reset().is_err());
    assert!(stage.set_bypass(true).is_err());
    assert!(stage.process(&mut pcm, 0, true).is_err());
}

#[test]
fn explicit_parent_cli_uses_interleaved_pcm_and_actual_tail_timing() {
    let bytes=br#"{"sampleRate":8000,"maxFrames":64,"blocks":[{"samples":[0.5,-0.25],"controls":{"presence":3,"stereoizer":5,"sshaper":true}},{"samples":[],"end":true}]}"#;
    let value = wire::render(bytes).unwrap();
    assert_eq!(value["ambience"], 1);
    assert_eq!(value["bank"], 1);
    assert_eq!(value["completeGraph"], false);
    assert_eq!(value["blocks"][0]["timing"]["validFrames"], 1);
    assert_eq!(value["blocks"][1]["timing"]["validFrames"], 64);
    assert_eq!(value["blocks"][1]["timing"]["end"], false);
    assert_eq!(value["blocks"][1]["samples"].as_array().unwrap().len(), 128);
    assert!(wire::render(br#"{"sampleRate":8000,"maxFrames":64,"blocks":[{"samples":[],"controls":{"ambience":2}}]}"#).is_err());
}
