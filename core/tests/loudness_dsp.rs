use base64::{Engine, engine::general_purpose::STANDARD};
use cumulus_core::effects::{
    dsp::{Format, LoudnessParameters, LoudnessStage, PlanarBlock},
    loudness::{TrackGain, fixed_gain},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{io::Read, sync::OnceLock};

#[derive(Deserialize)]
struct Corpus {
    conversion: Conversion,
    vectors: Vec<Vector>,
}
#[derive(Deserialize)]
struct Conversion {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    input: Value,
    output: f64,
}
#[derive(Deserialize)]
struct Vector {
    label: String,
    input_b64: String,
    output_b64: String,
}
fn corpus() -> &'static Corpus {
    static CORPUS: OnceLock<Corpus> = OnceLock::new();
    CORPUS.get_or_init(|| {
        let mut data = Vec::new();
        flate2::read::GzDecoder::new(&include_bytes!("fixtures/loudness-official.json.gz")[..])
            .take(16 * 1024 * 1024)
            .read_to_end(&mut data)
            .unwrap();
        serde_json::from_slice(&data).unwrap()
    })
}

#[test]
fn executed_m81_conversion_and_native_integer_boundary() {
    assert_eq!(corpus().conversion.cases.len(), 510);
    for case in &corpus().conversion.cases {
        let result = fixed_gain(&case.input);
        if (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&case.output) {
            assert_eq!(f64::from(result.unwrap()), case.output, "{:?}", case.input);
            let gain = TrackGain::from_volume_delta(&case.input).unwrap();
            assert_eq!(gain.is_some(), case.output != 0.0);
            if let Some(gain) = gain {
                assert_eq!(gain.decibels(), (case.output / 10000.0) as f32);
                assert_eq!(gain.parameters().peak, 0.0);
            }
        } else {
            // No claim about the official dictionary's out-of-range int cast.
            assert!(result.is_err(), "{:?}", case.input);
        }
    }
    assert!(fixed_gain(&json!(f64::MAX)).is_err());
}

struct Wire<'a>(&'a [u8]);
impl Wire<'_> {
    fn word(&mut self) -> [u8; 4] {
        let (word, rest) = self.0.split_at(4);
        self.0 = rest;
        word.try_into().unwrap()
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.word())
    }
    fn f32(&mut self) -> f32 {
        f32::from_le_bytes(self.word())
    }
}

#[test]
fn official_native_gain_peak_reference_transitions_and_planar_padding() {
    assert_eq!(corpus().vectors.len(), 159);
    for vector in &corpus().vectors {
        let input = STANDARD.decode(&vector.input_b64).unwrap();
        let expected = STANDARD.decode(&vector.output_b64).unwrap();
        let mut input = Wire(&input);
        let format = Format::new(input.u32() as f32, input.u32(), input.u32()).unwrap();
        let count = input.u32();
        let mut stage = LoudnessStage::new(format);
        let mut block = PlanarBlock::new(format);
        let mut output = Vec::new();
        for _ in 0..count {
            let frames = input.u32() as usize;
            let actions = input.u32();
            let bypass = input.u32() != 0;
            let parameters = LoudnessParameters {
                gain_db: input.f32(),
                peak: input.f32(),
            };
            let reference = input.f32();
            for sample in block.samples_mut() {
                *sample = input.f32();
            }
            if actions & 2 != 0 {
                stage.set_bypass(bypass).unwrap();
            }
            if actions & 1 != 0 {
                stage.set_parameters(parameters).unwrap();
            }
            if actions & 4 != 0 {
                stage.set_reference(reference).unwrap();
            }
            if actions & 8 != 0 {
                stage.reset();
            }
            let before = block.samples().to_vec();
            stage.process(&mut block, frames).unwrap();
            for channel in 0..format.channels() {
                let tail = channel * format.capacity() + frames;
                let end = (channel + 1) * format.capacity();
                assert_eq!(&block.samples()[tail..end], &before[tail..end]);
            }
            output.extend([
                stage.parameters().gain_db,
                stage.parameters().peak,
                stage.target_factor(),
                stage.previous_factor(),
                stage.reference_db(),
            ]);
            output.extend(block.samples());
        }
        assert!(input.0.is_empty());
        assert_eq!(output.len() * 4, expected.len());
        for (i, (actual, encoded)) in output.iter().zip(expected.chunks_exact(4)).enumerate() {
            let expected = f32::from_le_bytes(encoded.try_into().unwrap());
            // Native MSVC powf and platform libm can differ by a few ULPs.
            // Compare full state and every valid/padded sample, not only RMS.
            assert!(
                actual.is_finite() && (actual - expected).abs() <= 2e-6 * expected.abs().max(1.0),
                "{} sample {i}: {actual} != {expected}",
                vector.label
            );
        }
    }
}

#[test]
fn invalid_parameters_and_blocks_preserve_pcm_and_transition_history() {
    let format = Format::new(48000.0, 2, 7).unwrap();
    let mut stage = LoudnessStage::new(format);
    stage
        .set_parameters(LoudnessParameters {
            gain_db: 6.0,
            peak: 0.0,
        })
        .unwrap();
    stage.set_bypass(true).unwrap();
    let before = (
        stage.parameters(),
        stage.previous_factor(),
        stage.target_factor(),
    );
    for parameters in [
        LoudnessParameters {
            gain_db: f32::NAN,
            peak: 0.0,
        },
        LoudnessParameters {
            gain_db: 0.0,
            peak: f32::INFINITY,
        },
    ] {
        assert!(stage.set_parameters(parameters).is_err());
    }
    assert!(stage.set_reference(f32::NAN).is_err());
    assert_eq!(
        (
            stage.parameters(),
            stage.previous_factor(),
            stage.target_factor()
        ),
        before
    );
    let mut block = PlanarBlock::new(format);
    block.samples_mut().fill(f32::MAX);
    let samples = block.samples().to_vec();
    assert!(stage.process(&mut block, 7).is_err());
    assert_eq!(block.samples(), samples);
    assert_eq!(stage.previous_factor(), before.1);
    assert!(stage.process(&mut block, 8).is_err());
    let mut other = PlanarBlock::new(Format::new(48000.0, 1, 7).unwrap());
    assert!(stage.process(&mut other, 7).is_err());
    block.samples_mut()[0] = f32::NAN;
    assert!(stage.process(&mut block, 0).is_err());
    assert_eq!(stage.previous_factor(), before.1);
    block.samples_mut().fill(0.25);
    stage.process(&mut block, 0).unwrap();
    assert_eq!(stage.previous_factor(), 1.0);
    assert_eq!(block.samples(), [0.25; 14]);
    stage.process(&mut block, 7).unwrap();
    assert_eq!(block.samples(), [0.25; 14]);
}
