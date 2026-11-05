//! Synthetic original-node vectors; no official binary/assets at test runtime.
use cumulus_core::effects::dsp::*;
use serde::Deserialize;
use std::{io::Read, sync::OnceLock};
#[path = "../examples/support/stereo_zero_wire.rs"]
mod wire;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Block {
    frames: usize,
    bypass: bool,
    end: bool,
    presence: f32,
    stereoizer: f32,
    shaper: bool,
    reset: bool,
    input_bits: Vec<u32>,
    output_bits: Vec<u32>,
    state_bits: Vec<u32>,
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
            &include_bytes!("fixtures/effects-stereo-zero-official.json.gz")[..],
        )
        .take(16 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    })
}
#[cfg(target_arch = "x86_64")]
fn mxcsr() -> u32 {
    let mut value = 0u32;
    unsafe {
        std::arch::asm!("stmxcsr [{}]",in(reg)&mut value,options(nostack,preserves_flags));
    }
    value
}
#[cfg(target_arch = "x86_64")]
fn set_mxcsr(value: u32) {
    unsafe {
        std::arch::asm!("ldmxcsr [{}]",in(reg)&value,options(nostack,preserves_flags));
    }
}
struct Ftz(u32);
impl Ftz {
    fn new(enabled: u32) -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            let value = mxcsr();
            set_mxcsr((value & !0x8000) | if enabled == 1 { 0x8000 } else { 0 });
            Self(value)
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = enabled;
            Self(0)
        }
    }
}
impl Drop for Ftz {
    fn drop(&mut self) {
        #[cfg(target_arch = "x86_64")]
        set_mxcsr(self.0);
    }
}
fn snapshot_bits(stage: &StereoNoAmbienceStage) -> Vec<u32> {
    let value = stage.snapshot().unwrap();
    [value.side_gain, value.applied_presence]
        .into_iter()
        .chain(value.detector_history.into_iter().flatten())
        .map(f32::to_bits)
        .collect()
}
#[test]
fn reachable_bank_zero_pcm_reset_bypass_and_state_match_original() {
    assert_eq!(corpus().schema, "cumulus-se-no-ambience-v1");
    assert_eq!(corpus().cases.len(), 42);
    let mut count = 0;
    for case in &corpus().cases {
        let _ftz = Ftz::new(case.caller_ftz);
        let format = Format::new(case.rate as f32, 2, case.capacity).unwrap();
        let mut stage = StereoNoAmbienceStage::new(format).unwrap();
        let mut block = PlanarBlock::new(format);
        for (index, input) in case.blocks.iter().enumerate() {
            assert_eq!(input.input_bits.len(), format.sample_count());
            for (sample, bits) in block.samples_mut().iter_mut().zip(&input.input_bits) {
                *sample = f32::from_bits(*bits);
            }
            stage
                .set_controls(StereoNoAmbienceControls {
                    presence: input.presence,
                    stereoizer: input.stereoizer,
                    spectrum_shaper: input.shaper,
                })
                .unwrap();
            stage.set_bypass(input.bypass).unwrap();
            if input.reset {
                stage.reset().unwrap();
            }
            let result = stage.process(&mut block, input.frames, input.end).unwrap();
            assert_eq!(
                result,
                StereoNoAmbienceResult {
                    valid_frames: input.frames,
                    end: input.end
                }
            );
            let actual = block.samples()[..input.frames]
                .iter()
                .chain(&block.samples()[format.capacity()..format.capacity() + input.frames])
                .map(|f| f.to_bits())
                .collect::<Vec<_>>();
            assert_eq!(actual, input.output_bits, "{} block{index} PCM", case.name);
            assert_eq!(
                snapshot_bits(&stage),
                input.state_bits,
                "{} block{index} state",
                case.name
            );
            #[cfg(target_arch = "x86_64")]
            assert_eq!(
                u32::from(mxcsr() & 0x8000 != 0),
                case.caller_ftz,
                "{} caller FTZ",
                case.name
            );
            count += 1;
        }
    }
    assert_eq!(count, 378);
}
#[test]
fn explicit_component_cli_uses_real_interleaved_pcm_and_preserves_frames_eof() {
    let case = corpus()
        .cases
        .iter()
        .find(|c| c.rate == 48000 && c.capacity == 64 && c.caller_ftz == 0)
        .unwrap();
    let _ftz = Ftz::new(0);
    let blocks=case.blocks.iter().map(|b| {
        let samples=(0..b.frames).flat_map(|i| [f32::from_bits(b.input_bits[i]),f32::from_bits(b.input_bits[case.capacity as usize+i])]).collect::<Vec<_>>();
        serde_json::json!({"samples":samples,"controls":{"presence":b.presence,"stereoizer":b.stereoizer,"sshaper":b.shaper},"bypass":b.bypass,"reset":b.reset,"end":b.end})
    }).collect::<Vec<_>>();
    let request = serde_json::to_vec(
        &serde_json::json!({"sampleRate":case.rate,"maxFrames":case.capacity,"blocks":blocks}),
    )
    .unwrap();
    let reply = wire::render(&request).unwrap();
    assert_eq!(reply["ambience"], 0);
    assert_eq!(reply["completeGraph"], false);
    for (input, output) in case.blocks.iter().zip(reply["blocks"].as_array().unwrap()) {
        assert_eq!(output["timing"]["validFrames"], input.frames);
        assert_eq!(output["timing"]["end"], input.end);
        let actual = output["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| (v.as_f64().unwrap() as f32).to_bits())
            .collect::<Vec<_>>();
        let expected = (0..input.frames)
            .flat_map(|i| [input.output_bits[i], input.output_bits[input.frames + i]])
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
    assert!(wire::render(br#"{"sampleRate":48000,"maxFrames":64,"blocks":[{"samples":[],"controls":{"ambience":1}}]}"#).is_err());
}
#[test]
fn unsupported_shapes_controls_and_nonfinite_input_are_atomic() {
    for (rate, channels, capacity) in [
        (48000.0, 1, 64),
        (48000.0, 2, 1),
        (48000.0, 2, 7),
        (44100.5, 2, 64),
    ] {
        assert!(
            StereoNoAmbienceStage::new(Format::new(rate, channels, capacity).unwrap()).is_err()
        );
    }
    let format = Format::new(48000.0, 2, 64).unwrap();
    let mut stage = StereoNoAmbienceStage::new(format).unwrap();
    let mut block = PlanarBlock::new(format);
    let initial = snapshot_bits(&stage);
    for (presence, stereoizer) in [
        (f32::NAN, 0.0),
        (0.0, f32::INFINITY),
        (-10.1, 0.0),
        (0.0, -0.1),
        (0.0, 10.1),
    ] {
        assert!(
            stage
                .set_controls(StereoNoAmbienceControls {
                    presence,
                    stereoizer,
                    spectrum_shaper: true
                })
                .is_err()
        );
        assert_eq!(snapshot_bits(&stage), initial);
    }
    for frames in [0, 65] {
        assert!(stage.process(&mut block, frames, false).is_err());
        assert_eq!(snapshot_bits(&stage), initial);
    }
    block.samples_mut()[0] = f32::NAN;
    assert!(stage.process(&mut block, 64, false).is_err());
    assert_eq!(snapshot_bits(&stage), initial);
    block.samples_mut().fill(0.0);
    assert!(stage.process(&mut block, 64, false).is_ok());
    assert!(
        stage
            .process(
                &mut PlanarBlock::new(Format::new(48000.0, 2, 128).unwrap()),
                64,
                false
            )
            .is_err()
    );
}
#[test]
fn numerical_failure_silences_and_requires_recreation_instead_of_reset() {
    let format = Format::new(48000.0, 2, 64).unwrap();
    let mut stage = StereoNoAmbienceStage::new(format).unwrap();
    let mut block = PlanarBlock::new(format);
    block.samples_mut().fill(f32::MAX);
    assert!(stage.process(&mut block, 64, false).is_err());
    assert!(block.samples().iter().all(|v| *v == 0.0));
    assert!(stage.reset().is_err());
    assert!(stage.set_bypass(true).is_err());
    assert!(
        stage
            .set_controls(StereoNoAmbienceControls::default())
            .is_err()
    );
    assert!(stage.process(&mut block, 64, false).is_err());
}
#[test]
fn no_environment_bank_zero_presence_does_not_enable_the_disabled_main_filter() {
    let format = Format::new(48000.0, 2, 64).unwrap();
    let mut low = StereoNoAmbienceStage::new(format).unwrap();
    let mut high = StereoNoAmbienceStage::new(format).unwrap();
    low.set_controls(StereoNoAmbienceControls {
        presence: -10.0,
        stereoizer: 5.0,
        spectrum_shaper: true,
    })
    .unwrap();
    high.set_controls(StereoNoAmbienceControls {
        presence: 10.0,
        stereoizer: 5.0,
        spectrum_shaper: true,
    })
    .unwrap();
    for step in 0..4 {
        let samples = (0..64)
            .flat_map(|i| [((i + step * 64) as f32 * 0.1).sin() * 0.3, 0.1])
            .collect::<Vec<_>>();
        let mut a = PlanarBlock::new(format);
        a.load_interleaved(&samples).unwrap();
        let mut b = PlanarBlock::new(format);
        b.load_interleaved(&samples).unwrap();
        low.process(&mut a, 64, false).unwrap();
        high.process(&mut b, 64, false).unwrap();
        assert_eq!(a.samples(), b.samples());
    }
    assert_ne!(
        low.snapshot().unwrap().applied_presence,
        high.snapshot().unwrap().applied_presence
    );
}
