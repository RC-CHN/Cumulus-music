use base64::{Engine, engine::general_purpose::STANDARD};
use cumulus_core::effects::{
    dsp::*,
    material::{RotationCall, apply_rotation},
};
use serde::Deserialize;
use std::{io::Read, sync::OnceLock};

#[path = "../examples/support/effects_spatial_wire.rs"]
mod wire;

#[derive(Deserialize)]
struct Vector {
    mode: String,
    label: String,
    capacity: usize,
    valid_inputs: Vec<usize>,
    input_b64: String,
    output_b64: String,
}
#[derive(Deserialize)]
struct Corpus {
    responses_b64: String,
    vectors: Vec<Vector>,
}

fn corpus() -> &'static Corpus {
    static VALUE: OnceLock<Corpus> = OnceLock::new();
    VALUE.get_or_init(|| {
        let decoder = flate2::read::GzDecoder::new(
            &include_bytes!("fixtures/effects-spatial-official.json.gz")[..],
        );
        let mut bytes = Vec::new();
        decoder
            .take(64 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    })
}

fn responses() -> RotationResponses {
    RotationResponses::from_le_bytes(&STANDARD.decode(&corpus().responses_b64).unwrap()).unwrap()
}
fn float(bytes: &[u8], index: usize) -> f32 {
    f32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
}
fn word(bytes: &[u8], offset: usize) -> usize {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
}

fn check_mode(mode: &str, count: usize) {
    let tables = STANDARD.decode(&corpus().responses_b64).unwrap();
    let mut checked = 0;
    for vector in corpus().vectors.iter().filter(|case| case.mode == mode) {
        checked += 1;
        let expected = STANDARD.decode(&vector.output_b64).unwrap();
        let actual = wire::render(
            mode,
            &STANDARD.decode(&vector.input_b64).unwrap(),
            Some(&tables),
        )
        .unwrap();
        assert_eq!(expected.len(), actual.len());
        let record = match mode {
            "kernel" => 10264,
            "stage" => 12 + vector.capacity * 8,
            _ => (vector.capacity * 2 + 10) * 4,
        };
        assert_eq!(expected.len(), record * vector.valid_inputs.len());
        for (index, (a, b)) in actual
            .chunks_exact(record)
            .zip(expected.chunks_exact(record))
            .enumerate()
        {
            match mode {
                "kernel" => {
                    assert_eq!(&a[..8], &b[..8], "{} block {index} header", vector.label);
                    assert_eq!(
                        &a[2056..],
                        &b[2056..],
                        "{} block {index} response interpolation",
                        vector.label
                    );
                    for sample in 0..512 {
                        assert!(
                            (float(&a[8..2056], sample) - float(&b[8..2056], sample)).abs() <= 3e-6,
                            "kernel {} block {index}",
                            vector.label
                        );
                    }
                }
                "stage" => {
                    assert_eq!(
                        &a[..12],
                        &b[..12],
                        "{} block {index} phase/end",
                        vector.label
                    );
                    let valid = word(a, 0);
                    assert!(valid <= vector.capacity);
                    for ch in 0..2 {
                        for frame in 0..valid {
                            let position = ch * vector.capacity + frame;
                            assert!(
                                (float(&a[12..], position) - float(&b[12..], position)).abs()
                                    <= 3e-6,
                                "stage {} block {index}",
                                vector.label
                            );
                        }
                    }
                }
                _ => {
                    let valid = vector.valid_inputs[index];
                    for ch in 0..2 {
                        let start = ch * vector.capacity * 4;
                        assert_eq!(
                            &a[start..start + valid * 4],
                            &b[start..start + valid * 4],
                            "{mode} {} block {index} channel {ch}",
                            vector.label
                        );
                    }
                    assert_eq!(
                        &a[record - 40..],
                        &b[record - 40..],
                        "{mode} {} block {index} detector history",
                        vector.label
                    );
                }
            }
        }
    }
    assert_eq!(checked, count);
}

#[test]
fn original_rotation_kernel_phase_responses_and_pcm() {
    check_mode("kernel", 82);
}
#[test]
fn original_rotation_fifo_bypass_end_and_reset() {
    check_mode("stage", 10);
}
#[test]
fn original_multiband_internal_detector_streams() {
    check_mode("internal", 51);
}
#[test]
fn original_multiband_external_detector_streams() {
    check_mode("sidechain", 51);
}

fn settings() -> DynamicsSettings {
    DynamicsSettings {
        bands: [DynamicsBand {
            input_gain_db: 0.0,
            threshold_db: -18.0,
            ratio: 2.2,
            attack_seconds: 0.001,
            release_seconds: 0.05,
            minimum_gain_db: -24.0,
            output_gain_db: 0.0,
        }; 5],
        crossover_hz: [100.0, 350.0, 1500.0, 7500.0],
        output_gain_db: 0.0,
        process_lfe: true,
        link_channels: false,
        fourth_order: false,
        band_count: 5,
        input_gain_db: 0.0,
    }
}
fn block(format: Format) -> PlanarBlock {
    let mut result = PlanarBlock::new(format);
    for (i, value) in result.samples_mut().iter_mut().enumerate() {
        *value = (i as f32 * 0.04).sin() * 0.2;
    }
    result
}

#[test]
fn rotation_owns_tables_and_validates_format_and_period_before_mutation() {
    let original = responses();
    let owned_hash = original.sha256().to_owned();
    let format = Format::new(44100.0, 2, 256).unwrap();
    let mut stage = RotationStage::new(format, original.clone()).unwrap();
    assert!(RotationStage::new(Format::new(48000.0, 2, 256).unwrap(), original.clone()).is_err());
    assert!(RotationStage::new(Format::new(44100.0, 1, 256).unwrap(), original.clone()).is_err());
    assert!(RotationStage::new(Format::new(44100.0, 2, 13).unwrap(), original.clone()).is_err());
    assert!(RotationKernel::new(original.clone(), 360.0).is_err());
    assert!(RotationKernel::new(original.clone(), f32::NAN).is_err());
    drop(original);
    assert_eq!(owned_hash.len(), 64);
    for period in [-1.0, 0.0, 0.99] {
        assert!(!stage.set_period(period).unwrap());
    }
    assert!(stage.set_period(f32::NAN).is_err());
    assert!(stage.set_period(f32::INFINITY).is_err());
    assert!(
        apply_rotation(
            &mut stage,
            &[
                RotationCall::On { value: true },
                RotationCall::Velocity { value: f32::NAN }
            ]
        )
        .is_err()
    );
    let mut audio = block(format);
    let before = audio.samples().to_vec();
    assert!(stage.process(&mut audio, 257, false).is_err());
    assert_eq!(audio.samples(), before);
    assert_eq!(
        stage.process(&mut audio, 256, false).unwrap().valid_frames,
        256
    );
    assert_eq!(audio.samples(), before);
    apply_rotation(
        &mut stage,
        &[
            RotationCall::Velocity { value: 20.0 },
            RotationCall::On { value: true },
        ],
    )
    .unwrap();
    std::thread::spawn(move || {
        assert_eq!(
            stage.process(&mut audio, 256, false).unwrap().valid_frames,
            0
        );
        assert_eq!(
            stage.process(&mut audio, 256, false).unwrap().valid_frames,
            256
        );
        let result = stage.process(&mut audio, 0, true).unwrap();
        assert_eq!(result.valid_frames, 256);
        assert!(result.end);
        assert_eq!(stage.process(&mut audio, 0, true).unwrap().valid_frames, 0);
    })
    .join()
    .unwrap();
}

#[test]
fn rotation_rejects_invalid_tables_and_kernel_reset_preserves_phase() {
    assert!(RotationResponses::from_le_bytes(&[0; 16]).is_err());
    let mut bytes = STANDARD.decode(&corpus().responses_b64).unwrap();
    bytes[16..20].copy_from_slice(&f32::INFINITY.to_le_bytes());
    assert!(RotationResponses::from_le_bytes(&bytes).is_err());
    let format = Format::new(44100.0, 2, 256).unwrap();
    let mut kernel = RotationKernel::new(responses(), 359.5).unwrap();
    kernel.set_period(1.0).unwrap();
    let mut audio = block(format);
    assert_eq!(kernel.process(&mut audio, 256).unwrap(), 0);
    let phase = kernel.phase();
    assert!(phase < 359.5);
    kernel.reset();
    assert_eq!(kernel.phase(), phase);
    audio.samples_mut()[0] = f32::NAN;
    assert!(kernel.process(&mut audio, 256).is_err());
    assert_eq!(kernel.phase(), phase);
    audio = block(format);
    assert_eq!(kernel.process(&mut audio, 256).unwrap(), 0);
    assert_eq!(kernel.interpolated_responses().len(), 2052);
}

#[test]
fn dynamics_parameter_codec_and_coefficient_preflight_are_atomic() {
    let mut config = settings();
    config.fourth_order = true;
    let parameters = DynamicsParameters::new(config).unwrap();
    assert_eq!(parameters.settings(), config);
    assert_eq!(
        DynamicsParameters::new(parameters.settings())
            .unwrap()
            .as_le_bytes(),
        parameters.as_le_bytes()
    );
    assert!(DynamicsParameters::from_le_bytes(&parameters.as_le_bytes()[..170]).is_err());
    for (offset, value) in [(160, 2), (161, 2), (162, 2), (163, 0)] {
        let mut raw = *parameters.as_le_bytes();
        raw[offset] = value;
        assert!(DynamicsParameters::from_le_bytes(&raw).is_err());
    }
    let mut invalid = config;
    invalid.bands[0].ratio = f32::from_bits(1);
    assert!(DynamicsParameters::new(invalid).is_err());
    let format = Format::new(48000.0, 1, 13).unwrap();
    let mut stage = MultibandDynamics::new(format, &parameters).unwrap();
    let mut reference = MultibandDynamics::new(format, &parameters).unwrap();
    stage.process(&mut block(format), None, 13).unwrap();
    reference.process(&mut block(format), None, 13).unwrap();
    let before = stage.detector_history().unwrap();
    let mut invalid = config;
    invalid.crossover_hz[0] = f32::from_bits(1);
    invalid.fourth_order = false;
    let parameters = DynamicsParameters::new(invalid).unwrap();
    assert!(stage.configure(&parameters).is_err());
    assert_eq!(stage.detector_history().unwrap(), before);
    invalid.fourth_order = true;
    assert!(MultibandDynamics::new(format, &DynamicsParameters::new(invalid).unwrap()).is_err());
    let (mut a, mut b) = (block(format), block(format));
    stage.process(&mut a, None, 13).unwrap();
    reference.process(&mut b, None, 13).unwrap();
    assert_eq!(a.samples(), b.samples());
    assert_eq!(
        stage.detector_history().unwrap(),
        reference.detector_history().unwrap()
    );
}

#[test]
fn dynamics_rejects_bad_buffers_and_zero_frames_preserve_all_state() {
    let format = Format::new(48000.0, 1, 13).unwrap();
    let parameters = DynamicsParameters::new(settings()).unwrap();
    assert!(MultibandDynamics::new(Format::new(48000.0, 2, 13).unwrap(), &parameters).is_err());
    let mut processor = MultibandDynamics::new(format, &parameters).unwrap();
    let (mut main, mut side) = (block(format), block(format));
    let before = main.samples().to_vec();
    let history = processor.detector_history().unwrap();
    processor.process(&mut main, Some(&mut side), 0).unwrap();
    assert_eq!(main.samples(), before);
    assert_eq!(processor.detector_history().unwrap(), history);
    assert!(processor.process(&mut main, Some(&mut side), 14).is_err());
    let mut wrong = block(Format::new(44100.0, 1, 13).unwrap());
    assert!(processor.process(&mut main, Some(&mut wrong), 13).is_err());
    side.samples_mut()[3] = f32::NAN;
    assert!(processor.process(&mut main, Some(&mut side), 13).is_err());
    assert_eq!(main.samples(), before);
    assert_eq!(processor.detector_history().unwrap(), history);
}

#[test]
fn dynamics_external_detector_uses_first_sample_and_reset_clears_history() {
    let format = Format::new(44100.0, 1, 128).unwrap();
    let mut config = settings();
    config.band_count = 1;
    let parameters = DynamicsParameters::new(config).unwrap();
    let mut first = MultibandDynamics::new(format, &parameters).unwrap();
    let mut second = MultibandDynamics::new(format, &parameters).unwrap();
    for _ in 0..2 {
        let (mut a, mut b, mut side_a, mut side_b) =
            (block(format), block(format), block(format), block(format));
        side_a.samples_mut().fill(0.0);
        side_b.samples_mut().fill(0.8);
        side_a.samples_mut()[0] = 0.25;
        side_b.samples_mut()[0] = 0.25;
        first.process(&mut a, Some(&mut side_a), 128).unwrap();
        second.process(&mut b, Some(&mut side_b), 128).unwrap();
        assert_eq!(a.samples(), b.samples());
        assert_eq!(
            first.detector_history().unwrap(),
            second.detector_history().unwrap()
        );
    }
    assert!(
        first
            .detector_history()
            .unwrap()
            .iter()
            .any(|value| *value != 0.0)
    );
    first.reset().unwrap();
    assert_eq!(first.detector_history().unwrap(), [0.0; 10]);
}

#[test]
fn dynamics_extreme_amplification_has_defined_failure_and_is_quarantined() {
    let format = Format::new(48000.0, 1, 13).unwrap();
    let mut config = settings();
    config.band_count = 1;
    config.bands[0].ratio = 1e-37;
    config.bands[0].threshold_db = -200.0;
    config.bands[0].attack_seconds = 1e-30;
    let mut processor =
        MultibandDynamics::new(format, &DynamicsParameters::new(config).unwrap()).unwrap();
    let mut audio = block(format);
    audio.samples_mut().fill(0.5);
    assert!(processor.process(&mut audio, None, 13).is_err());
    assert!(audio.samples().iter().all(|value| *value == 0.0));
    processor.reset().unwrap();
    assert!(processor.process(&mut audio, None, 13).is_err());
}
